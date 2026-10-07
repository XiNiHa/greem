//! The barrier after each generation: settle what finished, ship it as one
//! payload, announce what that payload releases.

use crate::error::{GraphQLError, PathSegment};
use crate::exec::column::{Slot, TurnRange};
use crate::exec::payload::{
    CompletedEntry, Data, EntrySource, IncrementalEntry, PayloadKind, PendingEntry, Step,
};
use crate::exec::run::Loop;
use crate::exec::scope::{Activity, FieldState, Scope};
use crate::exec::settle;
use crate::exec::state::{ErrorBehavior, GroupId, GroupKind, GroupState, Groups, Shared};
use std::ops::ControlFlow;
use std::sync::atomic::Ordering;

fn walk_scopes_mut(
    scope: &mut Scope<'_>,
    path: &mut Vec<Step>,
    f: &mut dyn for<'a> FnMut(&[Step], &mut Scope<'a>),
) {
    f(path, scope);
    if scope.activity == Activity::Quiescent {
        return;
    }
    for fi in 0..scope.fields.len() {
        let FieldState::Done(column) = &mut scope.fields[fi] else {
            continue;
        };
        let field = column.field;
        for t in 0..column.turns.len() {
            for c in 0..column.turns[t].children.len() {
                path.push(Step::Child {
                    field,
                    turn: t as u32,
                    child: c as u32,
                });
                column.turns[t].children[c].with_dependent_mut(|_, s| walk_scopes_mut(s, path, f));
                path.pop();
            }
        }
    }
    for d in 0..scope.deferred.len() {
        if let Some(inner) = scope.deferred[d].scope_mut() {
            path.push(Step::Deferred(d as u32));
            walk_scopes_mut(inner, path, f);
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
            column.turns[*turn as usize].children[*child as usize]
                .with_dependent_mut(|_, inner| with_scope_at_mut(inner, rest, f))
        }
        Some((Step::Deferred(index), rest)) => {
            let inner = scope.deferred[*index as usize]
                .scope_mut()
                .expect("running deferred set on path");
            with_scope_at_mut(inner, rest, f)
        }
    }
}

pub(crate) struct Entries {
    pub pending: Vec<PendingEntry>,
    pub incremental: Vec<IncrementalEntry>,
    pub completed: Vec<CompletedEntry>,
}

/// The stream items that exist in a turn but have not shipped yet, as
/// `(path of the list, first index, count)`.
struct UnshippedItems(Vec<(Vec<PathSegment>, u32, u32)>);

impl UnshippedItems {
    fn collect(root: &mut Scope<'_>) -> Self {
        let mut items = Vec::new();
        walk_scopes_mut(root, &mut Vec::new(), &mut |_, scope| {
            let header = scope.shared.table.header(scope.meta.entry);
            for column in scope.columns() {
                if !column.stream.as_ref().is_some_and(|d| d.owns_groups()) {
                    continue;
                }
                for range in column.turns.iter().skip(1).flat_map(|turn| &turn.ranges) {
                    if !range.shipped {
                        let mut path = scope.meta.objects[range.object as usize].path.clone();
                        path.push(PathSegment::Key(
                            header.fields[column.field as usize].key.clone(),
                        ));
                        items.push((path, range.start_index, range.len));
                    }
                }
            }
        });
        UnshippedItems(items)
    }

    /// Whether the object at `path` sits at or beneath one of these items.
    fn contain(&self, path: &[PathSegment]) -> bool {
        self.0.iter().any(|(list, start, len)| {
            path.len() > list.len()
                && path[..list.len()] == list[..]
                && matches!(
                    path[list.len()],
                    PathSegment::Index(i) if (*start as usize..(*start + *len) as usize).contains(&i)
                )
        })
    }
}

/// The objects whose deferred field sets run under `g`.
fn group_roots(root: &mut Scope<'_>, g: GroupId) -> Vec<(Vec<Step>, u32)> {
    let mut found = Vec::new();
    walk_scopes_mut(root, &mut Vec::new(), &mut |path, scope| {
        if scope.set != 0 {
            for (o, &og) in scope.groups.iter().enumerate() {
                if og == g {
                    found.push((path.to_vec(), o as u32));
                }
            }
        }
    });
    found
}

/// Every error recorded beneath `roots`, in response order.
fn root_errors(root: &mut Scope<'_>, roots: &[(Vec<Step>, u32)]) -> Vec<GraphQLError> {
    // The walk lists a scope's objects consecutively; collect each scope
    // once with all of them so its errors come out field by field.
    let mut by_scope: Vec<(Vec<Step>, Vec<u32>)> = Vec::new();
    for (path, object) in roots {
        match by_scope.last_mut() {
            Some((last, objects)) if last == path => objects.push(*object),
            _ => by_scope.push((path.clone(), vec![*object])),
        }
    }
    let mut errs = settle::ErrorSink::default();
    for (path, objects) in &by_scope {
        with_scope_at_mut(root, path, &mut |scope| {
            settle::collect_errors(scope, objects, &mut errs)
        });
    }
    errs.sorted()
}

/// Null propagation over `roots`: the error that reaches their boundary, if any.
fn settle_roots(root: &mut Scope<'_>, roots: &[(Vec<Step>, u32)]) -> Option<Box<GraphQLError>> {
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
    roots: &[(Vec<Step>, u32)],
    mut errors: Vec<GraphQLError>,
    id: &str,
    group_path: &[PathSegment],
    out: &mut Entries,
) {
    for (path, object) in roots {
        let object = *object;
        with_scope_at_mut(root, path, &mut |scope| settle::mark_alive(scope, object));
        let object_path = with_scope_at_mut(root, path, &mut |scope| {
            scope.meta.objects[object as usize].path.clone()
        });
        out.incremental.push(IncrementalEntry {
            id: id.to_owned(),
            depth: object_path.len(),
            sub_path: sub_path(&object_path, group_path),
            errors: std::mem::take(&mut errors),
            source: EntrySource::Defer {
                path: path.clone(),
                object,
            },
        });
    }
}

/// Drops every shared field set none of whose member fragments can still
/// deliver it.
fn drop_orphaned_shared(groups: &mut Groups) {
    for g in 0..groups.list.len() as GroupId {
        let group = groups.get(g);
        let waiting = matches!(group.state, GroupState::Released | GroupState::Unreleased);
        if group.freed || !waiting {
            continue;
        }
        let GroupKind::Shared { members } = &group.kind else {
            continue;
        };
        if members.iter().all(|&m| groups.is_dead(m)) {
            groups.get_mut(g).state = GroupState::Dropped;
        }
    }
}

/// Fails every announced group whose enclosing fragment (its `after`
/// dependency) failed or was dropped: the client was told to expect it, so it
/// completes with that fragment's error. Repeats for chains of dependents.
fn fail_dependents(groups: &mut Groups, out: &mut Entries) {
    loop {
        let mut failed = false;
        for g in 0..groups.list.len() as GroupId {
            let group = groups.get(g);
            let GroupKind::Defer {
                after: Some(after), ..
            } = group.kind
            else {
                continue;
            };
            let waiting = matches!(group.state, GroupState::Released | GroupState::Unreleased);
            if group.freed || !group.announced || !waiting {
                continue;
            }
            if !matches!(
                groups.get(after).state,
                GroupState::Failed | GroupState::Dropped
            ) {
                continue;
            }
            let failure = groups.get(after).failure.clone();
            let id = groups.assign_wire_id(g).to_string();
            let group = groups.get_mut(g);
            group.state = GroupState::Failed;
            group.failure = failure.clone();
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
fn announce_children(
    root: &mut Scope<'_>,
    shipped: &[GroupId],
    groups: &mut Groups,
    out: &mut Entries,
) {
    // A stream group ships item by item: objects under an item that has not
    // shipped are not alive yet, and are decided when it does.
    let unshipped = UnshippedItems::collect(root);
    walk_scopes_mut(root, &mut Vec::new(), &mut |_, scope| {
        for object in 0..scope.meta.objects.len() {
            let alive = scope.alive[object];
            let pending_groups: Vec<GroupId> = scope.meta.objects[object]
                .pending
                .iter()
                .map(|(_, g)| *g)
                .collect();
            for g in pending_groups {
                let group = groups.get(g);
                if group.state != GroupState::Unreleased || group.announced || groups.is_dead(g) {
                    continue;
                }
                if !group.parent.is_some_and(|p| shipped.contains(&p)) {
                    continue;
                }
                if !alive {
                    // The parent payload nulled this object: its groups never run.
                    if !unshipped.contain(&scope.meta.objects[object].path) {
                        groups.get_mut(g).state = GroupState::Dropped;
                    }
                    continue;
                } // Its enclosing fragment already failed: it is never announced.
                if let GroupKind::Defer {
                    after: Some(after), ..
                } = group.kind
                    && matches!(
                        groups.get(after).state,
                        GroupState::Failed | GroupState::Dropped
                    )
                {
                    groups.get_mut(g).state = GroupState::Dropped;
                    continue;
                }

                let (path, label) = match &group.kind {
                    GroupKind::Defer { path, label, .. } => (path.clone(), label.clone()),
                    _ => continue,
                };
                let id = groups.assign_wire_id(g);
                groups.get_mut(g).announced = true;
                out.pending.push(PendingEntry {
                    id: id.to_string(),
                    path,
                    label,
                });
            }
        }
        for column in scope.columns() {
            let Some(driver) = column.stream.as_ref().filter(|d| d.owns_groups()) else {
                continue;
            };
            let parent_groups: Vec<(usize, GroupId)> =
                driver.groups().iter().copied().enumerate().collect();
            for (p, g) in parent_groups {
                let group = groups.get(g);
                if !matches!(group.kind, GroupKind::Stream { .. })
                    || group.announced
                    || group.state != GroupState::Unreleased
                {
                    continue;
                }
                if !group.parent.is_some_and(|parent| shipped.contains(&parent))
                    || groups.is_dead(g)
                {
                    continue;
                }
                let parent_object = driver.parent_object(p);
                if !scope.alive[parent_object as usize] {
                    if !unshipped.contain(&scope.meta.objects[parent_object as usize].path) {
                        groups.get_mut(g).state = GroupState::Dropped;
                    }
                    continue;
                }
                // The list itself was nulled (an item error reached it) or
                // never was a list: there is no position to stream into.
                if !matches!(column.level0[parent_object as usize], Slot::Items { .. }) {
                    groups.get_mut(g).state = GroupState::Dropped;
                    continue;
                }
                if driver.source_ended(p) {
                    groups.get_mut(g).state = GroupState::Completed;
                    continue;
                }
                let (path, label) = match &group.kind {
                    GroupKind::Stream { path, label, .. } => (path.clone(), label.clone()),
                    _ => unreachable!(),
                };
                let id = groups.assign_wire_id(g);
                groups.get_mut(g).announced = true;
                out.pending.push(PendingEntry {
                    id: id.to_string(),
                    path,
                    label,
                });
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
        out: Entries {
            pending: Vec::new(),
            incremental: Vec::new(),
            completed: Vec::new(),
        },
        initial: None,
        shipped: Vec::new(),
        ready_shared: Vec::new(),
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
    /// Shared field sets that settled at this barrier; each ships with the
    /// first member fragment that completes.
    ready_shared: Vec<GroupId>,
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
        let mut stop = self.groups.get(0).halted;
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
    }

    /// Released, non-stream groups in wire-id (announcement) order, so the
    /// output never depends on internal allocation order. Shared field sets
    /// have no wire id and sort ahead of the fragments.
    fn candidates(&self) -> Vec<GroupId> {
        let mut candidates: Vec<(Option<u32>, GroupId)> = self
            .groups
            .list
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                !g.freed
                    && g.state == GroupState::Released
                    && !matches!(g.kind, GroupKind::Stream { .. })
            })
            .map(|(i, g)| (g.wire_id, i as GroupId))
            .collect();
        candidates.sort_by_key(|(wire, id)| (wire.is_some(), *wire, *id));
        candidates.into_iter().map(|(_, id)| id).collect()
    }

    /// Completes or fails every released group whose work is done. Breaks
    /// with the operation's errors when the initial group halted.
    fn ship_groups(&mut self) -> ControlFlow<Vec<GraphQLError>> {
        for g in self.candidates() {
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
        }
        ControlFlow::Continue(())
    }

    fn readiness(&self, g: GroupId, is_initial: bool) -> Readiness {
        let halted = self.groups.get(g).halted;
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
                if self.groups.get(after).state != GroupState::Completed
        ) {
            return Readiness::Wait;
        }
        let shared_failure = sharing
            .iter()
            .find(|&&s| self.groups.get(s).state == GroupState::Failed)
            .map(|&s| self.groups.get(s).failure.clone());
        if let Some(failure) = shared_failure {
            return Readiness::SharedFailed(failure);
        }
        if halted {
            return Readiness::Ready { halted, sharing };
        }
        if self.root.is_live_for(g, self.groups) {
            return Readiness::Wait;
        }
        // A fragment completes together with the sets it shares.
        if sharing.iter().any(|s| {
            !self.ready_shared.contains(s)
                && matches!(
                    self.groups.get(*s).state,
                    GroupState::Released | GroupState::Unreleased
                )
        }) {
            return Readiness::Wait;
        }
        Readiness::Ready { halted, sharing }
    }

    /// Null propagation over `roots` under `Propagate`: the error that
    /// reaches their boundary, if any.
    fn settle(&mut self, roots: &[(Vec<Step>, u32)]) -> Option<Box<GraphQLError>> {
        if self.shared.behavior == ErrorBehavior::Propagate {
            settle_roots(self.root, roots)
        } else {
            None
        }
    }

    fn fail_with_shared(&mut self, g: GroupId, failure: Option<GraphQLError>) {
        let id = self.groups.assign_wire_id(g).to_string();
        let group = self.groups.get_mut(g);
        group.state = GroupState::Failed;
        group.failure = failure.clone();
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
            let errors = self.groups.get(g).failure.clone().into_iter().collect();
            self.groups.get_mut(g).state = GroupState::Failed;
            for other in self.groups.list.iter_mut().skip(1) {
                other.state = GroupState::Dropped;
            }
            return ControlFlow::Break(errors);
        }
        let roots = [(Vec::new(), 0)];
        let errors = root_errors(self.root, &roots);
        let failure = self.settle(&roots);
        self.groups.get_mut(g).state = GroupState::Completed;
        if failure.is_some() {
            self.initial = Some((Data::Null, errors));
        } else {
            settle::mark_alive(self.root, 0);
            self.shipped.push(g);
            self.initial = Some((Data::Root, errors));
        }
        ControlFlow::Continue(())
    }

    /// A shared field set settles on its own but ships with a member
    /// fragment: it fails here, or waits in `ready_shared` for one.
    fn settle_shared_set(&mut self, g: GroupId, halted: bool) {
        let failure = if halted {
            self.groups.get(g).failure.clone()
        } else {
            let roots = group_roots(self.root, g);
            self.settle(&roots).map(|error| *error)
        };
        if halted || failure.is_some() {
            let group = self.groups.get_mut(g);
            group.state = GroupState::Failed;
            group.failure = failure;
        } else {
            self.ready_shared.push(g);
        }
    }

    /// Fails a deferred fragment, or ships its data together with the shared
    /// field sets that settled, under one wire id.
    fn ship_fragment(&mut self, g: GroupId, halted: bool, sharing: Vec<GroupId>) {
        if halted {
            // The error that halted the group was captured when it was
            // recorded; it may still sit inside a pending column. It stays on
            // the group for the fragments that depend on it.
            let errors = self.groups.get(g).failure.clone().into_iter().collect();
            self.groups.get_mut(g).state = GroupState::Failed;
            let id = self.groups.assign_wire_id(g);
            self.out.completed.push(CompletedEntry {
                id: id.to_string(),
                errors,
            });
            return;
        }
        let roots = group_roots(self.root, g);
        let errors = root_errors(self.root, &roots);
        let failure = self.settle(&roots);
        let id = self.groups.assign_wire_id(g).to_string();
        if let Some(error) = failure {
            let group = self.groups.get_mut(g);
            group.state = GroupState::Failed;
            group.failure = Some((*error).clone());
            self.out.completed.push(CompletedEntry {
                id,
                errors: vec![*error],
            });
            return;
        }
        self.groups.get_mut(g).state = GroupState::Completed;
        let group_path = match &self.groups.get(g).kind {
            GroupKind::Defer { path, .. } => path.clone(),
            _ => Vec::new(),
        };
        self.shipped.push(g);
        ship_roots(self.root, &roots, errors, &id, &group_path, &mut self.out);
        // The shared sets that settled ship under this fragment's id.
        for s in sharing {
            if !self.ready_shared.contains(&s) {
                continue;
            }
            if self.groups.get(s).state != GroupState::Released {
                continue;
            }
            self.groups.get_mut(s).state = GroupState::Completed;
            self.shipped.push(s);
            let roots = group_roots(self.root, s);
            let errors = root_errors(self.root, &roots);
            ship_roots(self.root, &roots, errors, &id, &group_path, &mut self.out);
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
        walk_scopes_mut(self.root, &mut Vec::new(), &mut |path, scope| {
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
                        let parent = (0..driver.groups().len())
                            .find(|&p| driver.parent_object(p) == range.object)
                            .expect("parent");
                        let g = driver.groups()[parent];
                        let ready = turn
                            .children
                            .iter()
                            .all(|c| c.with_dependent(|_, s| !s.is_live_for(g, self.groups)));
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
        let driver_groups: Vec<GroupId> = with_scope_at_mut(self.root, path, &mut |scope| {
            let column = scope.column(field).expect("streamed column");
            column.stream.as_ref().expect("driver").groups().to_vec()
        });
        let mut halted = Vec::new();
        for g in driver_groups {
            let group = self.groups.get(g);
            if group.halted && group.state == GroupState::Released {
                let id = self.groups.assign_wire_id(g).to_string();
                let errors = self.groups.get_mut(g).failure.take().into_iter().collect();
                halted.push((g, id, errors));
            }
        }
        for (g, id, errors) in halted {
            self.fail_stream_group(path, field, g, id, errors);
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
            if self.groups.is_dead(g) || self.groups.get(g).state != GroupState::Released {
                continue;
            }
            let mut errs = settle::ErrorSink::default();
            with_scope_at_mut(self.root, path, &mut |scope| {
                let column = scope.column(field).expect("streamed column");
                settle::collect_range_errors(scope, column, t, range, &mut errs);
            });
            let errors = errs.sorted();
            let id = self.groups.assign_wire_id(g).to_string();
            // Propagation fails the group with the error that reached the boundary.
            let failure: Option<Vec<GraphQLError>> =
                if self.shared.behavior == ErrorBehavior::Propagate {
                    with_scope_at_mut(self.root, path, &mut |scope| {
                        let table = &scope.shared.table;
                        let meta = scope.meta;
                        let column = scope.columns_mut().find(|c| c.field == field).unwrap();
                        settle::settle_range(table, meta, column, t, range)
                            .err()
                            .map(|error| vec![*error])
                    })
                } else {
                    None
                };
            if let Some(errors) = failure {
                self.fail_stream_group(path, field, g, id, errors);
                continue;
            }
            with_scope_at_mut(self.root, path, &mut |scope| {
                let column = scope.columns_mut().find(|c| c.field == field).unwrap();
                settle::mark_alive_range(column, t, range);
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
    fn complete_streams(&mut self, path: &[Step], field: u32, handled: &[(usize, usize)]) {
        with_scope_at_mut(self.root, path, &mut |scope| {
            let column = scope.columns_mut().find(|c| c.field == field).unwrap();
            for &(t, ri) in handled {
                column.turns[t].ranges[ri].shipped = true;
            }
            for turn in column.turns.iter_mut().skip(1) {
                turn.shipped = turn.ranges.iter().all(|r| r.shipped);
            }
            let driver = column.stream.as_ref().expect("driver");
            for (p, &g) in driver.groups().iter().enumerate() {
                let group = self.groups.get(g);
                if !matches!(group.kind, GroupKind::Stream { .. })
                    || group.state != GroupState::Released
                {
                    continue;
                }
                let object = driver.parent_object(p);
                let pending_turns = column
                    .turns
                    .iter()
                    .skip(1)
                    .any(|turn| turn.ranges.iter().any(|r| !r.shipped && r.object == object));
                if driver.source_ended(p) && !pending_turns {
                    let id = self.groups.assign_wire_id(g).to_string();
                    self.groups.get_mut(g).state = GroupState::Completed;
                    self.out.completed.push(CompletedEntry {
                        id,
                        errors: Vec::new(),
                    });
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
        id: String,
        errors: Vec<GraphQLError>,
    ) {
        self.groups.get_mut(g).state = GroupState::Failed;
        self.out.completed.push(CompletedEntry { id, errors });
        with_scope_at_mut(self.root, path, &mut |scope| {
            let column = scope.columns_mut().find(|c| c.field == field).unwrap();
            if let Some(driver) = &mut column.stream {
                driver.drop_group(g);
            }
        });
    }

    /// Fails the announced groups whose enclosing fragment failed, drops the
    /// shared field sets nobody can deliver, and announces the children of
    /// whatever shipped.
    fn announce(&mut self) {
        fail_dependents(self.groups, &mut self.out);
        drop_orphaned_shared(self.groups);
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
    /// subsequent payload whenever there is an entry to send.
    fn finish(self, state: &mut Loop) -> Option<BarrierOutput> {
        let has_next = self.groups.list.iter().any(|g| {
            g.state == GroupState::Released || (g.state == GroupState::Unreleased && g.announced)
        }) || !self.out.pending.is_empty();
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
        }
        if self.out.pending.is_empty()
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
