//! The barrier after each generation: settle what finished, ship it as one
//! payload, announce what that payload releases.

use crate::error::{Error, GraphQLError, PathSegment};
use crate::exec::column::{Slot, TurnRange};
use crate::exec::payload::{
    CompletedEntry, Data, EntrySource, ErasedRoot, IncrementalEntry, Payload, PayloadKind,
    PendingEntry, Step,
};
use crate::exec::run::Loop;
use crate::exec::scope::{ANNOUNCE, Activity, FieldState, STREAM, Scope};
use crate::exec::settle;
use crate::exec::state::{ErrorBehavior, GroupId, GroupKind, GroupState, Groups, Root, Shared};
use smallvec::SmallVec;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Visits every scope flagged with `bit`, clearing it: a flag is raised on
/// a scope and all its ancestors, so the walk descends only where one is
/// set.
fn walk_flagged(
    scope: &mut Scope<'_>,
    bit: u8,
    path: &mut Vec<Step>,
    f: &mut dyn for<'a> FnMut(&[Step], &mut Scope<'a>),
) {
    if !scope.signal.take(bit) {
        return;
    }
    f(path, scope);
    if scope.activity == Activity::Quiescent {
        return;
    }
    for fi in 0..scope.fields.len() {
        let FieldState::Done(column) = &mut scope.fields[fi] else {
            continue;
        };
        let field = column.field;
        for (t, turn) in column.turns.iter_mut().enumerate() {
            turn.with_stored_mut(|stored| {
                for (c, child) in stored.children.iter_mut().enumerate() {
                    path.push(Step::Child {
                        field,
                        turn: t as u32,
                        child: c as u32,
                    });
                    child.with_dependent_mut(|_, s| walk_flagged(s, bit, path, f));
                    path.pop();
                }
            });
        }
    }
    for d in 0..scope.deferred.len() {
        if let Some(inner) = scope.deferred[d].scope_mut() {
            path.push(Step::Deferred(d as u32));
            walk_flagged(inner, bit, path, f);
            path.pop();
        }
    }
}

fn with_scope_at_mut<R>(
    scope: &mut Scope<'_>,
    path: &[Step],
    f: &mut dyn for<'a> FnMut(&mut Scope<'a>) -> R,
) -> R {
    match path.split_first() {
        None => f(scope),
        Some((Step::Child { field, turn, child }, rest)) => {
            let column = scope
                .columns_mut()
                .find(|c| c.field == *field)
                .expect("column on path");
            column.turns[*turn as usize]
                .with_child_mut(*child, |inner| with_scope_at_mut(inner, rest, f))
        }
        Some((Step::Deferred(index), rest)) => {
            let inner = scope.deferred[*index as usize]
                .scope_mut()
                .expect("running deferred set on path");
            with_scope_at_mut(inner, rest, f)
        }
    }
}

#[derive(Default)]
pub(crate) struct Entries {
    pub pending: Vec<PendingEntry>,
    pub incremental: Vec<IncrementalEntry>,
    pub completed: Vec<CompletedEntry>,
}

/// Every error recorded beneath `roots`, in response order, through the
/// barrier's sink.
fn root_errors(
    root: &mut Scope<'_>,
    roots: &[Root],
    errs: &mut settle::ErrorSink,
) -> Vec<GraphQLError> {
    // The roots list a scope's objects consecutively; collect each scope
    // once with all of them so its errors come out field by field.
    let mut at = 0;
    while at < roots.len() {
        let path = &roots[at].0;
        let end = at + roots[at..].iter().take_while(|(p, _)| p == path).count();
        // A group has one root per scope: fragments are per object.
        let objects: SmallVec<[u32; 1]> = roots[at..end].iter().map(|(_, o)| *o).collect();
        with_scope_at_mut(root, path, &mut |scope| {
            settle::collect_errors(scope, &objects, errs)
        });
        at = end;
    }
    errs.drain_sorted()
}

/// Null propagation over `roots` under `Propagate`: the error that reaches
/// their boundary, if any.
fn settle_roots(
    behavior: ErrorBehavior,
    root: &mut Scope<'_>,
    roots: &[Root],
) -> Option<Box<GraphQLError>> {
    if behavior != ErrorBehavior::Propagate {
        return None;
    }
    roots.iter().find_map(|(path, object)| {
        with_scope_at_mut(root, path, &mut |scope| {
            settle::settle_object(scope, *object)
        })
        .err()
    })
}

/// Marks `roots` alive and emits their incremental entries under `id`; the
/// first entry carries `errors`.
fn ship_roots(
    root: &mut Scope<'_>,
    roots: &[Root],
    mut errors: Vec<GraphQLError>,
    id: u32,
    group_path: &[PathSegment],
    out: &mut Entries,
    scratch: &mut Vec<settle::Target>,
) {
    for (path, object) in roots {
        let object = *object;
        let (depth, sub_path) = with_scope_at_mut(root, path, &mut |scope| {
            settle::mark_alive(scope, object, scratch);
            let meta = scope.meta;
            // A fragment's roots sit at its own path, so the entry needs
            // no subPath; only a root elsewhere has its path built.
            let sub_path = if meta.has_path(object, group_path) {
                Vec::new()
            } else {
                sub_path(&meta.path(object), group_path)
            };
            (meta.depth(object), sub_path)
        });
        out.incremental.push(IncrementalEntry {
            id,
            depth,
            sub_path,
            errors: std::mem::take(&mut errors),
            source: EntrySource::Defer {
                path: path.clone(),
                object,
            },
        });
    }
}

/// Settles what the groups that died since the last barrier take with
/// them: their announced dependents fail, the shared field sets nobody can
/// deliver any more are dropped, and whatever an ancestor's death made
/// reclaimable is queued for the sweep. Repeats while that kills more.
fn settle_dead(groups: &mut Groups, out: &mut Entries) {
    loop {
        let died = groups.take_died();
        if died.is_empty() {
            break;
        }
        // Everything beneath a dead group is dead too; collect it once, in
        // id order, so the entries come out as a scan would emit them.
        let mut affected: Vec<GroupId> = died.clone();
        for &g in &died {
            affected.extend(groups.descendants(g));
        }
        affected.sort_unstable();
        affected.dedup();
        for &g in &affected {
            groups.queue_reclaim(g);
        }
        fail_dependents(groups, &affected, out);
        drop_orphaned_shared(groups, &affected);
    }
}

/// Drops every shared field set none of whose member fragments can still
/// deliver it. Only the sets of `dead` groups can have become so.
fn drop_orphaned_shared(groups: &mut Groups, dead: &[GroupId]) {
    let mut sets: Vec<GroupId> = dead.iter().flat_map(|&g| groups.sharing(g)).collect();
    sets.sort_unstable();
    sets.dedup();
    for g in sets {
        let group = groups.get(g);
        if !matches!(
            group.state,
            GroupState::Unreleased | GroupState::Released | GroupState::Halted(_)
        ) {
            continue;
        }
        let GroupKind::Shared { members } = &group.kind else {
            continue;
        };
        if members.iter().all(|&m| groups.is_dead(m)) {
            groups.set_state(g, GroupState::Dropped);
        }
    }
}

/// Fails every announced group among `candidates` that can no longer
/// deliver because the fragment enclosing it (its `after` dependency) or an
/// ancestor failed or was dropped: the client was told to expect it, so it
/// completes with that group's error. Repeats for chains of dependents.
fn fail_dependents(groups: &mut Groups, candidates: &[GroupId], out: &mut Entries) {
    loop {
        let mut failed = false;
        for &g in candidates {
            let group = groups.get(g);
            let after = match group.kind {
                GroupKind::Defer { after, .. } => after,
                GroupKind::Stream { .. } => None,
                _ => continue,
            };
            if !matches!(
                group.state,
                GroupState::Announced | GroupState::Released | GroupState::Halted(_)
            ) {
                continue;
            }
            let ancestors = std::iter::successors(group.parent, |&p| groups.get(p).parent);
            let Some(failure) =
                after
                    .into_iter()
                    .chain(ancestors)
                    .find_map(|d| match &groups.get(d).state {
                        GroupState::Failed(failure) => Some(failure.clone()),
                        GroupState::Dropped => Some(None),
                        _ => None,
                    })
            else {
                continue;
            };
            let id = groups.assign_wire_id(g);
            groups.set_state(g, GroupState::Failed(failure.clone()));
            out.completed.push(CompletedEntry {
                id,
                errors: failure.into_iter().collect(),
            });
            failed = true;
        }
        if !failed {
            break;
        }
    }
}

/// Announces every unreleased group whose parent shipped at this barrier:
/// assigns wire ids and emits `pending` entries in tree order over alive objects.
///
/// Only the objects decided at this barrier are looked at: a group's
/// parent shipped when the object carrying it was just marked alive or
/// nulled. Objects under a stream item that has not shipped are decided
/// when it does.
fn announce_children(
    root: &mut Scope<'_>,
    shipped: &[GroupId],
    groups: &mut Groups,
    out: &mut Entries,
) {
    let shipped: HashSet<GroupId> = shipped.iter().copied().collect();
    walk_flagged(root, ANNOUNCE, &mut Vec::new(), &mut |_, scope| {
        let mut decided = std::mem::take(&mut scope.decided);
        if decided.is_empty() {
            return;
        }
        decided.sort_unstable();
        decided.dedup();
        #[cfg(debug_assertions)]
        crate::__private::OBJECTS_ANNOUNCED.fetch_add(decided.len(), Ordering::Relaxed);
        let meta = scope.meta;
        for &object in &decided {
            let object = object as usize;
            let alive = scope.alive[object];
            for &(_, g) in &meta.objects[object].pending {
                let group = groups.get(g);
                if !matches!(group.state, GroupState::Unreleased) || groups.is_dead(g) {
                    continue;
                }
                if !group.parent.is_some_and(|p| shipped.contains(&p)) {
                    continue;
                }
                if !alive {
                    // The parent payload nulled this object: its groups never run.
                    groups.set_state(g, GroupState::Dropped);
                    continue;
                } // Its enclosing fragment already failed: it is never announced.
                if let GroupKind::Defer {
                    after: Some(after), ..
                } = group.kind
                    && matches!(
                        groups.get(after).state,
                        GroupState::Failed(_) | GroupState::Dropped
                    )
                {
                    groups.set_state(g, GroupState::Dropped);
                    continue;
                }

                let (path, label) = match &group.kind {
                    GroupKind::Defer { path, label, .. } => (path.clone(), label.clone()),
                    _ => continue,
                };
                let id = groups.assign_wire_id(g);
                groups.set_state(g, GroupState::Announced);
                out.pending.push(PendingEntry { id, path, label });
            }
        }
        for column in scope.columns() {
            let Some(driver) = column.stream.as_ref().filter(|d| d.owns_groups()) else {
                continue;
            };
            for &parent_object in &decided {
                let Some(p) = driver.parent_of(parent_object) else {
                    continue;
                };
                let g = driver.group(p);
                let group = groups.get(g);
                if !matches!(group.kind, GroupKind::Stream { .. })
                    || !matches!(group.state, GroupState::Unreleased)
                {
                    continue;
                }
                if !group.parent.is_some_and(|parent| shipped.contains(&parent))
                    || groups.is_dead(g)
                {
                    continue;
                }
                if !scope.alive[parent_object as usize] {
                    groups.set_state(g, GroupState::Dropped);
                    continue;
                }
                // The list itself was nulled (an item error reached it) or
                // never was a list: there is no position to stream into.
                if !matches!(column.level0[parent_object as usize], Slot::Items { .. }) {
                    groups.set_state(g, GroupState::Dropped);
                    continue;
                }
                if driver.source_ended(p) {
                    groups.set_state(g, GroupState::Completed);
                    continue;
                }
                let (path, label) = match &group.kind {
                    GroupKind::Stream { path, label, .. } => (path.clone(), label.clone()),
                    _ => unreachable!(),
                };
                let id = groups.assign_wire_id(g);
                groups.set_state(g, GroupState::Announced);
                out.pending.push(PendingEntry { id, path, label });
            }
        }
    });
}

fn sub_path(object_path: &[PathSegment], group_path: &[PathSegment]) -> Vec<PathSegment> {
    if object_path.len() >= group_path.len() && object_path[..group_path.len()] == *group_path {
        object_path[group_path.len()..].to_vec()
    } else {
        object_path.to_vec()
    }
}

/// What one barrier hands to the sink.
pub(crate) struct BarrierOutput {
    pub kind: PayloadKind,
    pub data: Data,
    pub errors: Vec<GraphQLError>,
    pub entries: Entries,
    pub has_next: Option<bool>,
}

impl BarrierOutput {
    pub fn into_payload(self, root: &dyn ErasedRoot) -> Payload<'_> {
        Payload {
            root: Some(root),
            kind: self.kind,
            data: self.data,
            errors: self.errors,
            pending: self.entries.pending,
            incremental: self.entries.incremental,
            completed: self.entries.completed,
            has_next: self.has_next,
        }
    }
}

/// What ships when nothing can make progress, which only an executor bug
/// causes. The response ends either way: `data: null` before the initial
/// payload, otherwise a failed `completed` entry for every announced group
/// still open.
pub(crate) fn stalled(groups: &mut Groups, initial_shipped: bool) -> BarrierOutput {
    let error = GraphQLError::from_error(
        &Error::framework("execution stalled", "EXECUTION_STALLED"),
        Vec::new(),
        Vec::new(),
    );
    if !initial_shipped {
        return BarrierOutput {
            kind: PayloadKind::Initial,
            data: Data::Null,
            errors: vec![error],
            entries: Entries::default(),
            has_next: None,
        };
    }
    let mut open: Vec<GroupId> = groups
        .iter()
        .filter(|(_, g)| {
            matches!(g.kind, GroupKind::Defer { .. } | GroupKind::Stream { .. })
                && matches!(
                    g.state,
                    GroupState::Announced | GroupState::Released | GroupState::Halted(_)
                )
        })
        .map(|(id, _)| id)
        .collect();
    open.sort_by_key(|&g| groups.get(g).wire_id);
    let mut entries = Entries::default();
    for g in open {
        let id = groups.assign_wire_id(g);
        groups.set_state(g, GroupState::Failed(Some(error.clone())));
        entries.completed.push(CompletedEntry {
            id,
            errors: vec![error.clone()],
        });
    }
    BarrierOutput {
        kind: PayloadKind::Subsequent,
        data: Data::Absent,
        errors: Vec::new(),
        entries,
        has_next: Some(false),
    }
}

/// Runs the barrier after a generation. Returns the payload's pieces; the
/// caller builds the borrowed `Payload` and calls the sink.
pub(crate) fn barrier(
    root: &mut Scope<'_>,
    shared: &Shared,
    state: &mut Loop,
) -> Option<BarrierOutput> {
    let mut groups = shared.groups();
    let mut barrier = Barrier {
        root,
        shared,
        groups: &mut groups,
        out: Entries::default(),
        initial: None,
        shipped: Vec::new(),
        errs: settle::ErrorSink::default(),
        scratch: Vec::new(),
    };
    barrier.close_serial_root();
    if let ControlFlow::Break(errors) = barrier.ship_groups() {
        state.done = true;
        return Some(BarrierOutput {
            kind: PayloadKind::Initial,
            data: Data::Null,
            errors,
            entries: barrier.out,
            has_next: None,
        });
    }
    barrier.ship_stream_turns();
    barrier.announce();
    barrier.order_entries();
    barrier.finish(state)
}

/// One barrier's working state, threaded through its phases.
struct Barrier<'r, 'a> {
    root: &'r mut Scope<'a>,
    shared: &'r Shared,
    /// Held for the whole barrier: nothing it calls takes the lock itself.
    groups: &'r mut Groups,
    out: Entries,
    /// The initial payload's data and errors, once the initial group settled.
    initial: Option<(Data, Vec<GraphQLError>)>,
    /// Groups whose data shipped at this barrier; their children are announced.
    shipped: Vec<GroupId>,
    /// Reused across the entries of one barrier.
    errs: settle::ErrorSink,
    scratch: Vec<settle::Target>,
}

/// Whether a released group can complete at this barrier.
enum Readiness {
    Wait,
    /// A field set it shares failed: it fails with that set's error.
    SharedFailed(Option<GraphQLError>),
    Ready {
        /// It ships at once: its pending work is abandoned, not awaited.
        halted: bool,
        /// The shared field sets it takes part in.
        sharing: Vec<GroupId>,
    },
}

impl Barrier<'_, '_> {
    /// Serial mutation roots: closes the finished root field before group 0
    /// is checked, and stops the chain if it halted or failed to settle.
    fn close_serial_root(&mut self) {
        let root = &mut *self.root;
        if !root.meta.serial || root.cursor >= root.fields.len() {
            return;
        }
        let cursor = root.cursor;
        let done = matches!(root.fields[cursor], FieldState::Done(_))
            && !root.column_live_for(cursor as u32, 0, self.groups);
        if !done {
            return;
        }
        let mut stop = matches!(self.groups.get(0).state, GroupState::Halted(_));
        if self.shared.behavior == ErrorBehavior::Propagate {
            let table = &self.shared.table;
            let meta = root.meta;
            let FieldState::Done(column) = &mut root.fields[cursor] else {
                unreachable!()
            };
            if settle::settle_column(table, meta, column, 0).is_err() {
                stop = true;
            }
        }
        root.cursor = if stop { root.fields.len() } else { cursor + 1 };
        root.release_work(self.groups);
        // The next root field is polled from the next generation.
        root.signal.raise(crate::exec::scope::POLL);
    }

    /// The order a scan of the table would examine candidates in: wire-id
    /// (announcement) order, so the output never depends on internal
    /// allocation order. Shared field sets have no wire id and sort ahead
    /// of the fragments.
    fn candidate_key(&self, g: GroupId) -> Option<(bool, Option<u32>, GroupId)> {
        let group = self.groups.get(g);
        (matches!(group.state, GroupState::Released | GroupState::Halted(_))
            && !matches!(group.kind, GroupKind::Stream { .. }))
        .then_some((group.wire_id.is_some(), group.wire_id, g))
    }

    /// Completes or fails every released group whose work is done. Breaks
    /// with the operation's errors when the initial group halted.
    ///
    /// Only the groups queued for examination are looked at, in candidate
    /// order. A group queued while an earlier one is handled joins this
    /// barrier when it sorts after it, as a scan would reach it, and waits
    /// for the next one otherwise.
    fn ship_groups(&mut self) -> ControlFlow<Vec<GraphQLError>> {
        let mut queue: BinaryHeap<Reverse<(bool, Option<u32>, GroupId)>> = BinaryHeap::new();
        for g in self.groups.take_examine() {
            if let Some(key) = self.candidate_key(g) {
                queue.push(Reverse(key));
            }
        }
        let mut later = Vec::new();
        while let Some(Reverse(key)) = queue.pop() {
            let g = key.2;
            // Queued again while in the queue, or handled by an earlier
            // group: its state has moved on.
            if self.candidate_key(g) != Some(key) {
                continue;
            }
            #[cfg(debug_assertions)]
            crate::__private::GROUPS_EXAMINED.fetch_add(1, Ordering::Relaxed);
            let kind = &self.groups.get(g).kind;
            let is_initial = matches!(kind, GroupKind::Initial);
            let is_shared = matches!(kind, GroupKind::Shared { .. });
            match self.readiness(g, is_initial) {
                Readiness::Wait => {}
                Readiness::SharedFailed(failure) => self.fail_with_shared(g, failure),
                Readiness::Ready { halted, .. } if is_initial => self.ship_initial(g, halted)?,
                Readiness::Ready { halted, .. } if is_shared => self.settle_shared_set(g, halted),
                Readiness::Ready { halted, sharing } => self.ship_fragment(g, halted, sharing),
            }
            for queued in self.groups.take_examine() {
                match self.candidate_key(queued) {
                    Some(next) if next > key => queue.push(Reverse(next)),
                    Some(_) => later.push(queued),
                    None => {}
                }
            }
        }
        for g in later {
            self.groups.queue_examine(g);
        }
        ControlFlow::Continue(())
    }

    fn readiness(&mut self, g: GroupId, is_initial: bool) -> Readiness {
        let halted = matches!(self.groups.get(g).state, GroupState::Halted(_));
        let sharing = if is_initial {
            Vec::new()
        } else {
            self.groups.sharing(g)
        };
        // A nested fragment never completes, successfully or not, before
        // the fragment enclosing it has; if that one failed,
        // `fail_dependents` fails this one with its error. A fragment whose
        // fields are all shared has no deferred set of its own to hold it back.
        if matches!(
            self.groups.get(g).kind,
            GroupKind::Defer { after: Some(after), .. }
                if !matches!(self.groups.get(after).state, GroupState::Completed)
        ) {
            return Readiness::Wait;
        }
        let shared_failure = sharing
            .iter()
            .find_map(|&s| match &self.groups.get(s).state {
                GroupState::Failed(failure) => Some(failure.clone()),
                _ => None,
            });
        if let Some(failure) = shared_failure {
            return Readiness::SharedFailed(failure);
        }
        if halted {
            return Readiness::Ready { halted, sharing };
        }
        if self.groups.is_live(g) {
            return Readiness::Wait;
        }
        // A fragment completes together with the sets it shares.
        if sharing.iter().any(|s| {
            !self.groups.is_settled(*s)
                && matches!(
                    self.groups.get(*s).state,
                    GroupState::Unreleased | GroupState::Released | GroupState::Halted(_)
                )
        }) {
            return Readiness::Wait;
        }
        Readiness::Ready { halted, sharing }
    }

    fn fail_with_shared(&mut self, g: GroupId, failure: Option<GraphQLError>) {
        let id = self.groups.assign_wire_id(g);
        self.groups
            .set_state(g, GroupState::Failed(failure.clone()));
        self.out.completed.push(CompletedEntry {
            id,
            errors: failure.into_iter().collect(),
        });
    }

    /// Settles the whole tree for the initial payload. Breaks with the
    /// operation's errors when the initial group halted.
    fn ship_initial(&mut self, g: GroupId, halted: bool) -> ControlFlow<Vec<GraphQLError>> {
        if halted {
            // The error that halted the group was captured when it was
            // recorded; it may still sit inside a pending column.
            let error = self.groups.fail_halted(g);
            let others: Vec<GroupId> = self.groups.iter().map(|(id, _)| id).collect();
            for id in others {
                if id != g {
                    self.groups.set_state(id, GroupState::Dropped);
                }
            }
            return ControlFlow::Break(vec![error]);
        }
        let roots = [(Arc::from(Vec::new()), 0)];
        let errors = root_errors(self.root, &roots, &mut self.errs);
        let failure = settle_roots(self.shared.behavior, self.root, &roots);
        self.groups.set_state(g, GroupState::Completed);
        if failure.is_some() {
            self.initial = Some((Data::Null, errors));
        } else {
            settle::mark_alive(self.root, 0, &mut self.scratch);
            self.shipped.push(g);
            self.initial = Some((Data::Root, errors));
        }
        ControlFlow::Continue(())
    }

    /// A shared field set settles on its own but ships with a member
    /// fragment: it fails here, or is marked settled for one to ship.
    fn settle_shared_set(&mut self, g: GroupId, halted: bool) {
        if halted {
            self.groups.fail_halted(g);
            return;
        }
        let failure = settle_roots(self.shared.behavior, self.root, self.groups.roots_of(g));
        match failure {
            Some(error) => self.groups.set_state(g, GroupState::Failed(Some(*error))),
            None => self.groups.mark_settled(g),
        }
    }

    /// Fails a deferred fragment, or ships its data together with the shared
    /// field sets that settled, under one wire id.
    fn ship_fragment(&mut self, g: GroupId, halted: bool, sharing: Vec<GroupId>) {
        if halted {
            // The error that halted the group was captured when it was
            // recorded; it may still sit inside a pending column.
            let error = self.groups.fail_halted(g);
            let id = self.groups.assign_wire_id(g);
            self.out.completed.push(CompletedEntry {
                id,
                errors: vec![error],
            });
            return;
        }
        let errors = root_errors(self.root, self.groups.roots_of(g), &mut self.errs);
        let failure = settle_roots(self.shared.behavior, self.root, self.groups.roots_of(g));
        let id = self.groups.assign_wire_id(g);
        if let Some(error) = failure {
            self.groups
                .set_state(g, GroupState::Failed(Some((*error).clone())));
            self.out.completed.push(CompletedEntry {
                id,
                errors: vec![*error],
            });
            return;
        }
        self.groups.set_state(g, GroupState::Completed);
        let group_path: Arc<[PathSegment]> = match &self.groups.get(g).kind {
            GroupKind::Defer { path, .. } => path.clone(),
            _ => Arc::from([]),
        };
        self.shipped.push(g);
        ship_roots(
            self.root,
            self.groups.roots_of(g),
            errors,
            id,
            &group_path,
            &mut self.out,
            &mut self.scratch,
        );
        // Its payload announces the groups beneath it. Those on objects it
        // delivered were decided above; a nested fragment on the very object
        // carrying it, or one whose fields all ran in a shared set, sits on
        // an object decided earlier, so that object is listed again.
        for (path, object) in self.groups.child_carriers(g) {
            with_scope_at_mut(self.root, &path, &mut |scope| {
                if scope.decided.is_empty() {
                    scope.signal.raise(ANNOUNCE);
                }
                scope.decided.push(object);
            });
        }
        // The shared sets that settled ship under this fragment's id.
        for s in sharing {
            if !self.groups.is_settled(s) {
                continue;
            }
            if !matches!(self.groups.get(s).state, GroupState::Released) {
                continue;
            }
            self.groups.set_state(s, GroupState::Completed);
            self.shipped.push(s);
            let errors = root_errors(self.root, self.groups.roots_of(s), &mut self.errs);
            ship_roots(
                self.root,
                self.groups.roots_of(s),
                errors,
                id,
                &group_path,
                &mut self.out,
                &mut self.scratch,
            );
        }
        self.out.completed.push(CompletedEntry {
            id,
            errors: Vec::new(),
        });
    }

    /// Per streamed column: ships the item ranges whose subtrees finished,
    /// fails halted streams, completes streams with nothing left to ship.
    fn ship_stream_turns(&mut self) {
        if !self.shared.has_streams.load(Ordering::Relaxed) {
            return;
        }
        let mut columns: Vec<(Vec<Step>, u32)> = Vec::new();
        walk_flagged(self.root, STREAM, &mut Vec::new(), &mut |path, scope| {
            for column in scope.columns() {
                if column.stream.as_ref().is_some_and(|d| d.owns_groups()) {
                    columns.push((path.to_vec(), column.field));
                }
            }
        });
        for (path, field) in columns {
            let ranges = self.ready_ranges(&path, field);
            self.fail_halted_streams(&path, field);
            let shipped = self.ship_ranges(&path, field, ranges);
            self.complete_streams(&path, field, &shipped);
        }
        self.groups.clear_halted();
    }

    /// The unshipped item ranges of one streamed column that may ship now,
    /// as `(turn, range index, range, group)`.
    fn ready_ranges(
        &mut self,
        path: &[Step],
        field: u32,
    ) -> Vec<(usize, usize, TurnRange, GroupId)> {
        // A parent's items ship once nothing under them is still live for that
        // parent's stream group; nested streams and deferred groups announced
        // under it are released by that very payload, so they never hold it back.
        let mut ranges: Vec<(usize, usize, TurnRange, GroupId, bool)> =
            with_scope_at_mut(self.root, path, &mut |scope| {
                let column = scope.column(field).expect("streamed column");
                let driver = column.stream.as_ref().expect("driver");
                let mut ranges = Vec::new();
                for (t, turn) in column.turns.iter().enumerate().skip(1) {
                    if turn.shipped {
                        continue;
                    }
                    for (ri, range) in turn.ranges.iter().enumerate() {
                        if range.shipped {
                            continue;
                        }
                        let g = driver.group(range.parent as usize);
                        let ready = !turn.any_child(|s| s.is_live_for(g, self.groups));
                        ranges.push((t, ri, *range, g, ready));
                    }
                }
                ranges
            });
        // A parent's items ship in list order: a finished range waits for the
        // earlier ranges of the same parent, whichever turn slot holds them.
        // Dead groups deliver nothing, so their ranges are not ordered.
        ranges.sort_by_key(|(_, _, range, _, _)| (range.object, range.start_index));
        let mut waiting = None;
        ranges
            .into_iter()
            .filter_map(|(t, ri, range, g, ready)| {
                if self.groups.is_dead(g) {
                    return ready.then_some((t, ri, range, g));
                }
                if waiting == Some(range.object) {
                    return None;
                }
                if !ready {
                    waiting = Some(range.object);
                    return None;
                }
                Some((t, ri, range, g))
            })
            .collect()
    }

    /// HALT fails a group with the error that halted it as soon as it is
    /// released, whether its items made a turn yet or their children are
    /// still pending.
    fn fail_halted_streams(&mut self, path: &[Step], field: u32) {
        if self.groups.halted().is_empty() {
            return;
        }
        // A halted stream group names its driver parent; it is this column's
        // when that parent carries it.
        let groups = &*self.groups;
        let mut mine: Vec<(usize, GroupId)> = with_scope_at_mut(self.root, path, &mut |scope| {
            let column = scope.column(field).expect("streamed column");
            let driver = column.stream.as_ref().expect("driver");
            groups
                .halted()
                .iter()
                .filter_map(|&g| match groups.get(g).kind {
                    GroupKind::Stream { parent, .. }
                        if (parent as usize) < driver.parent_count()
                            && driver.group(parent as usize) == g =>
                    {
                        Some((parent as usize, g))
                    }
                    _ => None,
                })
                .collect()
        });
        mine.sort_unstable();
        for (_, g) in mine {
            let GroupState::Halted(error) = &self.groups.get(g).state else {
                continue;
            };
            let error = error.clone();
            let id = self.groups.assign_wire_id(g);
            self.fail_stream_group(path, field, g, id, error);
        }
    }

    /// Ships each ready range as one incremental entry, or fails its stream
    /// when propagation reaches the boundary. Returns every range it handled,
    /// as `(turn, range index)`.
    fn ship_ranges(
        &mut self,
        path: &[Step],
        field: u32,
        ranges: Vec<(usize, usize, TurnRange, GroupId)>,
    ) -> Vec<(usize, usize)> {
        let mut handled = Vec::new();
        for (t, ri, range, g) in ranges {
            handled.push((t, ri));
            if self.groups.is_dead(g) || !matches!(self.groups.get(g).state, GroupState::Released) {
                continue;
            }
            let errs = &mut self.errs;
            with_scope_at_mut(self.root, path, &mut |scope| {
                let column = scope.column(field).expect("streamed column");
                settle::collect_range_errors(scope, column, t, range, errs);
            });
            let errors = self.errs.drain_sorted();
            let id = self.groups.assign_wire_id(g);
            // Propagation fails the group with the error that reached the boundary.
            let failure: Option<GraphQLError> = if self.shared.behavior == ErrorBehavior::Propagate
            {
                with_scope_at_mut(self.root, path, &mut |scope| {
                    let table = &scope.shared.table;
                    let meta = scope.meta;
                    let column = scope.columns_mut().find(|c| c.field == field).unwrap();
                    settle::settle_range(table, meta, column, t, range)
                        .err()
                        .map(|error| *error)
                })
            } else {
                None
            };
            if let Some(error) = failure {
                self.fail_stream_group(path, field, g, id, error);
                continue;
            }
            let scratch = &mut self.scratch;
            with_scope_at_mut(self.root, path, &mut |scope| {
                let column = scope.columns_mut().find(|c| c.field == field).unwrap();
                settle::mark_alive_range(column, t, range, scratch);
            });
            let depth = match &self.groups.get(g).kind {
                GroupKind::Stream { path, .. } => path.len() + 1,
                _ => 0,
            };
            self.out.incremental.push(IncrementalEntry {
                id,
                depth,
                sub_path: Vec::new(),
                errors,
                source: EntrySource::Items {
                    path: path.to_vec(),
                    field,
                    turn: t as u32,
                    start_slot: range.start_slot,
                    len: range.len,
                },
            });
            self.shipped.push(g);
        }
        handled
    }

    /// Marks the handled ranges and the turns they empty as shipped, and
    /// completes every stream whose source ended with nothing left to ship.
    /// Only the parents the driver queued are examined: those whose source
    /// ended or whose last range shipped since the previous barrier.
    fn complete_streams(&mut self, path: &[Step], field: u32, handled: &[(usize, usize)]) {
        with_scope_at_mut(self.root, path, &mut |scope| {
            let column = scope.columns_mut().find(|c| c.field == field).unwrap();
            let driver = column.stream.as_mut().expect("driver");
            for &(t, ri) in handled {
                let range = &mut column.turns[t].ranges[ri];
                range.shipped = true;
                driver.range_shipped(range.parent as usize);
            }
            for turn in column.turns.iter_mut().skip(1) {
                turn.shipped = turn.ranges.iter().all(|r| r.shipped);
            }
            for p in driver.take_completable() {
                let g = driver.group(p);
                let group = self.groups.get(g);
                if !matches!(group.kind, GroupKind::Stream { .. }) {
                    continue;
                }
                match group.state {
                    GroupState::Released => {
                        let id = self.groups.assign_wire_id(g);
                        self.groups.set_state(g, GroupState::Completed);
                        self.out.completed.push(CompletedEntry {
                            id,
                            errors: Vec::new(),
                        });
                    }
                    GroupState::Unreleased | GroupState::Announced => {
                        driver.queue_completion(p);
                    }
                    _ => {}
                }
            }
        });
    }

    /// Fails a stream group: state, its `completed` entry, and dropping the source.
    fn fail_stream_group(
        &mut self,
        path: &[Step],
        field: u32,
        g: GroupId,
        id: u32,
        error: GraphQLError,
    ) {
        self.groups
            .set_state(g, GroupState::Failed(Some(error.clone())));
        self.out.completed.push(CompletedEntry {
            id,
            errors: vec![error],
        });
        let GroupKind::Stream { parent, .. } = self.groups.get(g).kind else {
            unreachable!("stream group")
        };
        let groups = &mut *self.groups;
        with_scope_at_mut(self.root, path, &mut |scope| {
            let column = scope.columns_mut().find(|c| c.field == field).unwrap();
            if let Some(driver) = &mut column.stream {
                driver.drop_parent(parent as usize, groups);
            }
        });
    }

    /// Fails the announced groups whose enclosing fragment failed, drops the
    /// shared field sets nobody can deliver, and announces the children of
    /// whatever shipped.
    fn announce(&mut self) {
        settle_dead(self.groups, &mut self.out);
        if !self.shipped.is_empty() {
            announce_children(self.root, &self.shipped, self.groups, &mut self.out);
        }
    }

    /// A payload may carry a stream's items and a fragment deferred on or
    /// beneath one of them; clients apply entries in order, so the entries that
    /// create positions go first: shallower paths, and items before data.
    fn order_entries(&mut self) {
        self.out.incremental.sort_by_key(|entry| {
            (
                entry.depth,
                matches!(entry.source, EntrySource::Defer { .. }),
            )
        });
    }

    /// The initial payload once the initial group settled; after it, a
    /// subsequent payload whenever there is an entry to send, and always for
    /// the last one.
    fn finish(self, state: &mut Loop) -> Option<BarrierOutput> {
        let has_next = self.groups.any_open() || !self.out.pending.is_empty();
        if !state.initial_shipped {
            let (data, errors) = self.initial?;
            state.initial_shipped = true;
            let has_next = if self.shared.incremental && has_next {
                Some(true)
            } else {
                None
            };
            if has_next.is_none() {
                state.done = true;
            }
            return Some(BarrierOutput {
                kind: PayloadKind::Initial,
                data,
                errors,
                entries: self.out,
                has_next,
            });
        }
        if !has_next {
            state.done = true;
        } else if self.out.pending.is_empty()
            && self.out.incremental.is_empty()
            && self.out.completed.is_empty()
        {
            return None;
        }
        Some(BarrierOutput {
            kind: PayloadKind::Subsequent,
            data: Data::Absent,
            errors: Vec::new(),
            entries: self.out,
            has_next: Some(has_next),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defer() -> GroupKind {
        GroupKind::Defer {
            usage: 0,
            label: None,
            path: Arc::from([]),
            after: None,
        }
    }

    fn stream() -> GroupKind {
        GroupKind::Stream {
            node: 0,
            label: None,
            path: Arc::from([]),
            parent: 0,
        }
    }

    #[test]
    fn a_stall_fails_every_announced_group_still_open() {
        let mut groups = Groups::new();
        fn announced(groups: &mut Groups, kind: GroupKind, state: GroupState) -> GroupId {
            let g = groups.alloc(kind, 0);
            groups.assign_wire_id(g);
            groups.set_state(g, state);
            g
        }
        let released = announced(&mut groups, defer(), GroupState::Released);
        let completed = announced(&mut groups, stream(), GroupState::Completed);
        let halted = announced(
            &mut groups,
            stream(),
            GroupState::Halted(GraphQLError::from_error(
                &Error::new("halted"),
                Vec::new(),
                Vec::new(),
            )),
        );
        let pending = announced(&mut groups, defer(), GroupState::Announced);
        let unannounced = groups.alloc(defer(), 0);
        let shared = groups.alloc(
            GroupKind::Shared {
                members: vec![released],
            },
            0,
        );
        groups.set_state(shared, GroupState::Released);

        let output = stalled(&mut groups, true);
        assert_eq!(output.kind, PayloadKind::Subsequent);
        assert_eq!(output.has_next, Some(false));
        let failed: Vec<String> = output
            .entries
            .completed
            .iter()
            .map(|entry| entry.id.to_string())
            .collect();
        let failed: Vec<&str> = failed.iter().map(String::as_str).collect();
        assert_eq!(failed, ["0", "2", "3"]);
        for entry in &output.entries.completed {
            assert_eq!(entry.errors[0].message, "execution stalled");
        }
        for g in [released, halted, pending] {
            assert!(matches!(groups.get(g).state, GroupState::Failed(Some(_))));
        }
        assert!(matches!(groups.get(completed).state, GroupState::Completed));
        assert!(matches!(
            groups.get(unannounced).state,
            GroupState::Unreleased
        ));
        assert!(matches!(groups.get(shared).state, GroupState::Released));
    }

    #[test]
    fn a_stall_before_the_initial_payload_nulls_the_data() {
        let output = stalled(&mut Groups::new(), false);
        assert_eq!(output.kind, PayloadKind::Initial);
        assert!(matches!(output.data, Data::Null));
        assert_eq!(output.has_next, None);
        assert_eq!(output.errors.len(), 1);
        assert_eq!(output.errors[0].message, "execution stalled");
    }
}
