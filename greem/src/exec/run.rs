//! The generation loop: poll the whole tree one generation, run the barrier
//! (null pass, payload, release), advance, repeat.

use crate::error::{GraphQLError, PathSegment};
use crate::exec::column::{Slot, TurnRange};
use crate::exec::payload::{
    CompletedEntry, Data, EntrySource, IncrementalEntry, Payload, PayloadKind, PendingEntry, Step,
};
use crate::exec::scope::{FieldState, Scope};
use crate::exec::settle;
use crate::exec::state::{ErrorBehavior, GroupId, GroupKind, GroupState, Shared};
use std::sync::atomic::Ordering;
use std::task::Poll;

pub(crate) fn walk_scopes_mut(
    scope: &mut Scope<'_>,
    path: &mut Vec<Step>,
    f: &mut dyn for<'a> FnMut(&[Step], &mut Scope<'a>),
) {
    f(path, scope);
    if scope.quiescent {
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
    for p in 0..scope.parked.len() {
        if let Some(inner) = &mut scope.parked[p].scope {
            path.push(Step::Parked(p as u32));
            walk_scopes_mut(inner, path, f);
            path.pop();
        }
    }
}

pub(crate) fn with_scope_at_mut<R>(
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
        Some((Step::Parked(index), rest)) => {
            let inner = scope.parked[*index as usize]
                .scope
                .as_mut()
                .expect("released parked scope on path");
            with_scope_at_mut(inner, rest, f)
        }
    }
}

/// Fails a stream group: state, its `completed` entry, and dropping the source.
#[allow(clippy::too_many_arguments)]
fn fail_stream_group(
    root: &mut Scope<'_>,
    path: &[Step],
    field: u32,
    shared: &Shared,
    out: &mut Entries,
    g: GroupId,
    id: String,
    errors: Vec<GraphQLError>,
) {
    shared.groups.lock().unwrap().get_mut(g).state = GroupState::Failed;
    out.completed.push(CompletedEntry { id, errors });
    with_scope_at_mut(root, path, &mut |scope| {
        let column = scope.columns_mut().find(|c| c.field == field).unwrap();
        if let Some(driver) = &mut column.stream {
            driver.drop_group(g);
        }
    });
}

pub(crate) struct Entries {
    pending: Vec<PendingEntry>,
    incremental: Vec<IncrementalEntry>,
    completed: Vec<CompletedEntry>,
}

/// The stream items that exist in a turn but have not shipped yet, as
/// `(path of the list, first index, count)`.
struct UnshippedItems(Vec<(Vec<PathSegment>, u32, u32)>);

impl UnshippedItems {
    fn collect(root: &mut Scope<'_>, shared: &Shared) -> Self {
        let mut items = Vec::new();
        walk_scopes_mut(root, &mut Vec::new(), &mut |_, scope| {
            let header = shared.table.header(scope.meta.entry);
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
fn drop_orphaned_shared(shared: &Shared) {
    let mut groups = shared.groups.lock().unwrap();
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
fn fail_dependents(shared: &Shared, out: &mut Entries) {
    let mut groups = shared.groups.lock().unwrap();
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
    shared: &Shared,
    out: &mut Entries,
) {
    // A stream group ships item by item: objects under an item that has not
    // shipped are not alive yet, and are decided when it does.
    let unshipped = UnshippedItems::collect(root, shared);
    walk_scopes_mut(root, &mut Vec::new(), &mut |_, scope| {
        for object in 0..scope.meta.objects.len() {
            let alive = scope.alive[object];
            let pending_groups: Vec<GroupId> = scope.meta.objects[object]
                .pending
                .iter()
                .map(|(_, g)| *g)
                .collect();
            let mut groups = shared.groups.lock().unwrap();
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
            let mut groups = shared.groups.lock().unwrap();
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

pub(crate) struct Loop {
    pub generation: u32,
    pub initial_shipped: bool,
    pub done: bool,
}

/// Runs the barrier after a generation. Returns the payload's pieces; the
/// caller builds the borrowed `Payload` and calls the sink.
pub(crate) fn barrier(
    root: &mut Scope<'_>,
    shared: &Shared,
    state: &mut Loop,
) -> Option<BarrierOutput> {
    let behavior = shared.behavior;
    let mut out = Entries {
        pending: Vec::new(),
        incremental: Vec::new(),
        completed: Vec::new(),
    };
    let mut initial: Option<(Data, Vec<GraphQLError>)> = None;
    let mut shipped: Vec<GroupId> = Vec::new();

    // Serial mutation roots: close the finished chain before checking group 0.
    if root.meta.serial && root.cursor < root.fields.len() {
        let cursor = root.cursor;
        let done = matches!(root.fields[cursor], FieldState::Done(_))
            && !root.column_live_for(cursor as u32, 0);
        if done {
            let halted = shared.groups.lock().unwrap().get(0).halted;
            let mut stop = halted;
            if behavior == ErrorBehavior::Propagate {
                let table = &shared.table;
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
    }

    // Released, non-stream groups in wire-id (announcement) order, so the
    // output never depends on internal allocation order.
    let candidates: Vec<(GroupId, bool)> = {
        let groups = shared.groups.lock().unwrap();
        let mut candidates: Vec<(Option<u32>, GroupId, bool)> = groups
            .list
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                !g.freed
                    && g.state == GroupState::Released
                    && !matches!(g.kind, GroupKind::Stream { .. })
            })
            .map(|(i, g)| {
                (
                    g.wire_id,
                    i as GroupId,
                    matches!(g.kind, GroupKind::Initial),
                )
            })
            .collect();
        candidates.sort_by_key(|(wire, id, _)| (wire.is_some(), *wire, *id));
        candidates
            .into_iter()
            .map(|(_, id, initial)| (id, initial))
            .collect()
    };
    // Shared field sets that settled at this barrier; each ships with the
    // first member fragment that completes. Without a wire id they sort
    // ahead of the fragments.
    let mut ready_shared: Vec<GroupId> = Vec::new();
    for (g, is_initial) in candidates {
        let (halted, is_shared, sharing) = {
            let groups = shared.groups.lock().unwrap();
            (
                groups.get(g).halted,
                matches!(groups.get(g).kind, GroupKind::Shared { .. }),
                if is_initial {
                    Vec::new()
                } else {
                    groups.sharing(g)
                },
            )
        };
        // A nested fragment never completes, successfully or not, before
        // the fragment enclosing it has; if that one failed,
        // `fail_dependents` fails this one with its error. A fragment whose
        // fields are all shared has no parked scope to hold it back.
        let awaits_enclosing = {
            let groups = shared.groups.lock().unwrap();
            matches!(
                groups.get(g).kind,
                GroupKind::Defer { after: Some(after), .. }
                    if groups.get(after).state != GroupState::Completed
            )
        };
        if awaits_enclosing {
            continue;
        }
        // A field set this fragment shares failed: the fragment fails with it.
        let shared_failure = {
            let groups = shared.groups.lock().unwrap();
            sharing
                .iter()
                .find(|&&s| groups.get(s).state == GroupState::Failed)
                .map(|&s| groups.get(s).failure.clone())
        };
        if let Some(failure) = shared_failure {
            let mut groups = shared.groups.lock().unwrap();
            let id = groups.assign_wire_id(g).to_string();
            let group = groups.get_mut(g);
            group.state = GroupState::Failed;
            group.failure = failure.clone();
            out.completed.push(CompletedEntry {
                id,
                errors: failure.into_iter().collect(),
            });
            continue;
        }
        // A halted group ships at once: its pending work is abandoned, not awaited.
        if !halted && root.is_live_for(g) {
            continue;
        }
        // A fragment completes together with the sets it shares.
        let awaits_shared = {
            let groups = shared.groups.lock().unwrap();
            sharing.iter().any(|s| {
                !ready_shared.contains(s)
                    && matches!(
                        groups.get(*s).state,
                        GroupState::Released | GroupState::Unreleased
                    )
            })
        };
        if !halted && awaits_shared {
            continue;
        }
        let roots: Vec<(Vec<Step>, u32)> = if is_initial {
            vec![(Vec::new(), 0)]
        } else {
            group_roots(root, g)
        };
        if is_shared {
            let failure = if halted {
                shared.groups.lock().unwrap().get(g).failure.clone()
            } else if behavior == ErrorBehavior::Propagate {
                settle_roots(root, &roots).map(|error| *error)
            } else {
                None
            };
            if halted || failure.is_some() {
                let mut groups = shared.groups.lock().unwrap();
                let group = groups.get_mut(g);
                group.state = GroupState::Failed;
                group.failure = failure;
            } else {
                ready_shared.push(g);
            }
            continue;
        }
        let mut errors = root_errors(root, &roots);
        if halted {
            let mut groups = shared.groups.lock().unwrap();
            // The error that halted the group was captured when it was
            // recorded; it may still sit inside a pending column. It stays on
            // the group for the fragments that depend on it.
            errors = groups.get(g).failure.clone().into_iter().collect();
            if is_initial {
                groups.get_mut(g).state = GroupState::Failed;
                for other in groups.list.iter_mut().skip(1) {
                    other.state = GroupState::Dropped;
                }
                drop(groups);
                state.done = true;
                return Some(BarrierOutput {
                    kind: PayloadKind::Initial,
                    data: Data::Null,
                    errors,
                    entries: out,
                    has_next: None,
                });
            }
            groups.get_mut(g).state = GroupState::Failed;
            let id = groups.assign_wire_id(g);
            out.completed.push(CompletedEntry {
                id: id.to_string(),
                errors,
            });
            continue;
        }
        let failure = if behavior == ErrorBehavior::Propagate {
            settle_roots(root, &roots)
        } else {
            None
        };
        if is_initial {
            shared.groups.lock().unwrap().get_mut(g).state = GroupState::Completed;
            if failure.is_some() {
                initial = Some((Data::Null, errors));
            } else {
                settle::mark_alive(root, 0);
                shipped.push(g);
                initial = Some((Data::Root, errors));
            }
            continue;
        }
        let mut groups = shared.groups.lock().unwrap();
        let id = groups.assign_wire_id(g).to_string();
        if let Some(error) = failure {
            let group = groups.get_mut(g);
            group.state = GroupState::Failed;
            group.failure = Some((*error).clone());
            out.completed.push(CompletedEntry {
                id,
                errors: vec![*error],
            });
            continue;
        }
        groups.get_mut(g).state = GroupState::Completed;
        let group_path = match &groups.get(g).kind {
            GroupKind::Defer { path, .. } => path.clone(),
            _ => Vec::new(),
        };
        drop(groups);
        shipped.push(g);
        ship_roots(root, &roots, errors, &id, &group_path, &mut out);
        // The shared sets that settled ship under this fragment's id.
        for s in sharing {
            if !ready_shared.contains(&s) {
                continue;
            }
            {
                let mut groups = shared.groups.lock().unwrap();
                if groups.get(s).state != GroupState::Released {
                    continue;
                }
                groups.get_mut(s).state = GroupState::Completed;
            }
            shipped.push(s);
            let roots = group_roots(root, s);
            let errors = root_errors(root, &roots);
            ship_roots(root, &roots, errors, &id, &group_path, &mut out);
        }
        out.completed.push(CompletedEntry {
            id,
            errors: Vec::new(),
        });
    }

    // Stream turns.
    let mut turns: Vec<(Vec<Step>, u32)> = Vec::new();
    if shared
        .has_streams
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        walk_scopes_mut(root, &mut Vec::new(), &mut |path, scope| {
            for column in scope.columns() {
                if column.stream.as_ref().is_some_and(|d| d.owns_groups()) {
                    turns.push((path.to_vec(), column.field));
                }
            }
        });
    }
    for (path, field) in turns {
        // A parent's items ship once nothing under them is still live for that
        // parent's stream group; nested streams and deferred groups announced
        // under it are released by that very payload, so they never hold it back.
        let ranges: Vec<(usize, usize, TurnRange, GroupId, bool)> =
            with_scope_at_mut(root, &path, &mut |scope| {
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
                            .all(|c| c.with_dependent(|_, s| !s.is_live_for(g)));
                        ranges.push((t, ri, *range, g, ready));
                    }
                }
                ranges
            });
        // A parent's items ship in list order: a finished range waits for the
        // earlier ranges of the same parent, whichever turn slot holds them.
        // Dead groups deliver nothing, so their ranges are not ordered.
        let ranges: Vec<(usize, usize, TurnRange, GroupId)> = {
            let mut ranges = ranges;
            ranges.sort_by_key(|(_, _, range, _, _)| (range.object, range.start_index));
            let groups = shared.groups.lock().unwrap();
            let mut waiting = None;
            ranges
                .into_iter()
                .filter_map(|(t, ri, range, g, ready)| {
                    if groups.is_dead(g) {
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
        };
        // HALT fails a group with the error that halted it as soon as it is
        // released, whether its items made a turn yet or their children are
        // still pending.
        let halted: Vec<(GroupId, String, Vec<GraphQLError>)> = {
            let driver_groups: Vec<GroupId> = with_scope_at_mut(root, &path, &mut |scope| {
                let column = scope.column(field).expect("streamed column");
                column.stream.as_ref().expect("driver").groups().to_vec()
            });
            let mut groups = shared.groups.lock().unwrap();
            let mut halted = Vec::new();
            for g in driver_groups {
                let group = groups.get(g);
                if group.halted && group.state == GroupState::Released {
                    let id = groups.assign_wire_id(g).to_string();
                    let errors = groups.get_mut(g).failure.take().into_iter().collect();
                    halted.push((g, id, errors));
                }
            }
            halted
        };
        for (g, id, errors) in halted {
            fail_stream_group(root, &path, field, shared, &mut out, g, id, errors);
        }
        let mut shipped_ranges = Vec::new();
        for (t, ri, range, g) in ranges {
            shipped_ranges.push((t, ri));
            let (dead, released) = {
                let groups = shared.groups.lock().unwrap();
                (
                    groups.is_dead(g),
                    groups.get(g).state == GroupState::Released,
                )
            };
            if dead || !released {
                continue;
            }
            let mut errs = settle::ErrorSink::default();
            with_scope_at_mut(root, &path, &mut |scope| {
                let column = scope.column(field).expect("streamed column");
                settle::collect_range_errors(scope, column, t, range, &mut errs);
            });
            let errors = errs.sorted();
            let id = shared.groups.lock().unwrap().assign_wire_id(g).to_string();
            // Propagation fails the group with the error that reached the boundary.
            let failure: Option<Vec<GraphQLError>> = if behavior == ErrorBehavior::Propagate {
                with_scope_at_mut(root, &path, &mut |scope| {
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
                fail_stream_group(root, &path, field, shared, &mut out, g, id, errors);
                continue;
            }
            with_scope_at_mut(root, &path, &mut |scope| {
                let column = scope.columns_mut().find(|c| c.field == field).unwrap();
                settle::mark_alive_range(column, t, range);
            });
            let depth = match &shared.groups.lock().unwrap().get(g).kind {
                GroupKind::Stream { path, .. } => path.len() + 1,
                _ => 0,
            };
            out.incremental.push(IncrementalEntry {
                id,
                depth,
                sub_path: Vec::new(),
                errors,
                source: EntrySource::Items {
                    path: path.clone(),
                    field,
                    turn: t as u32,
                    start_slot: range.start_slot,
                    len: range.len,
                },
            });
            shipped.push(g);
        }
        with_scope_at_mut(root, &path, &mut |scope| {
            let column = scope.columns_mut().find(|c| c.field == field).unwrap();
            for &(t, ri) in &shipped_ranges {
                column.turns[t].ranges[ri].shipped = true;
            }
            for turn in column.turns.iter_mut().skip(1) {
                turn.shipped = turn.ranges.iter().all(|r| r.shipped);
            }
            let driver = column.stream.as_ref().expect("driver");
            let mut groups = shared.groups.lock().unwrap();
            for (p, &g) in driver.groups().iter().enumerate() {
                let group = groups.get(g);
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
                    let id = groups.assign_wire_id(g).to_string();
                    groups.get_mut(g).state = GroupState::Completed;
                    out.completed.push(CompletedEntry {
                        id,
                        errors: Vec::new(),
                    });
                }
            }
        });
    }

    fail_dependents(shared, &mut out);
    drop_orphaned_shared(shared);
    if !shipped.is_empty() {
        announce_children(root, &shipped, shared, &mut out);
    }
    // A payload may carry a stream's items and a fragment deferred on or
    // beneath one of them; clients apply entries in order, so the entries that
    // create positions go first: shallower paths, and items before data.
    out.incremental.sort_by_key(|entry| {
        (
            entry.depth,
            matches!(entry.source, EntrySource::Defer { .. }),
        )
    });

    let has_next = {
        let groups = shared.groups.lock().unwrap();
        groups.list.iter().any(|g| {
            g.state == GroupState::Released || (g.state == GroupState::Unreleased && g.announced)
        }) || !out.pending.is_empty()
    };
    if !state.initial_shipped {
        let (data, errors) = initial?;
        state.initial_shipped = true;
        let has_next = if shared.incremental && has_next {
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
            entries: out,
            has_next,
        });
    }
    if !has_next {
        state.done = true;
    }
    if out.pending.is_empty() && out.incremental.is_empty() && out.completed.is_empty() {
        return None;
    }
    Some(BarrierOutput {
        kind: PayloadKind::Subsequent,
        data: Data::Absent,
        errors: Vec::new(),
        entries: out,
        has_next: Some(has_next),
    })
}

/// After the sink returns: release announced groups, start parked scopes,
/// make stream turns, retire shipped turns.
pub(crate) fn advance(root: &mut Scope<'_>, shared: &Shared) -> bool {
    let mut changed = false;
    {
        let mut groups = shared.groups.lock().unwrap();
        for group in &mut groups.list {
            if group.state == GroupState::Unreleased && group.announced {
                group.state = GroupState::Released;
                // The next barrier can ship it, even when it starts no work
                // itself (a fragment whose only content is a nested defer).
                changed = true;
            }
        }
        // A shared field set runs as soon as one member fragment may.
        for g in 0..groups.list.len() as GroupId {
            let group = groups.get(g);
            if group.freed || group.state != GroupState::Unreleased {
                continue;
            }
            let GroupKind::Shared { members } = &group.kind else {
                continue;
            };
            if members
                .iter()
                .any(|&m| groups.is_released(m) && !groups.is_dead(m))
            {
                groups.get_mut(g).state = GroupState::Released;
                changed = true;
            }
        }
    }
    advance_scope(root, shared, &mut changed);
    root.clear_fresh();
    // Retired turns and finished scopes dropped their group references above;
    // terminal groups nobody references any more give their slots back.
    shared.groups.lock().unwrap().sweep();
    changed
}

/// Advances one scope's subtree; a subtree that changed nothing and has no
/// work left becomes quiescent, so later polls and barriers skip it.
fn advance_scope(scope: &mut Scope<'_>, shared: &Shared, changed: &mut bool) {
    // Every object here belongs to a dead group: nothing it produces can be
    // delivered, so its pending futures are never polled again.
    if !scope.quiescent && scope.all_dead(shared) {
        scope.quiescent = true;
        return;
    }
    let mut local = false;
    advance_scope_inner(scope, shared, &mut local);
    if local {
        scope.quiescent = false;
        *changed = true;
    } else if scope.is_finished() {
        scope.quiescent = true;
    }
}

fn advance_scope_inner(scope: &mut Scope<'_>, shared: &Shared, changed: &mut bool) {
    if scope.quiescent {
        return;
    }
    for i in 0..scope.fields.len() {
        let FieldState::Done(column) = &mut scope.fields[i] else {
            continue;
        };
        for turn in &mut column.turns {
            for child in &mut turn.children {
                child.with_dependent_mut(|_, s| advance_scope(s, shared, changed));
            }
        }
        for turn in column.turns.iter_mut().skip(1) {
            if turn.shipped
                && !turn.retired
                && turn
                    .children
                    .iter()
                    .all(|c| c.with_dependent(|_, s| s.is_finished()))
            {
                turn.retire();
            }
        }
        if let Some(mut driver) = column.stream.take() {
            let released = {
                let groups = shared.groups.lock().unwrap();
                driver
                    .groups()
                    .iter()
                    .all(|&g| groups.is_released(g) || groups.is_dead(g))
            };
            if released {
                driver.release();
                if driver.make_turn(column) {
                    *changed = true;
                }
            }
            column.stream = Some(driver);
        }
    }
    for parked in &mut scope.parked {
        if let Some(inner) = &mut parked.scope {
            advance_scope(inner, shared, changed);
            continue;
        }
        if parked.dropped || parked.start.is_none() {
            continue;
        }
        let (all_dead, ready) = {
            let mut groups = shared.groups.lock().unwrap();
            // A group whose `after` dependency failed or was dropped is
            // dropped too; one already announced was failed by the barrier.
            for &g in &parked.groups {
                if let GroupKind::Defer {
                    after: Some(after), ..
                } = groups.get(g).kind
                    && matches!(
                        groups.get(after).state,
                        GroupState::Failed | GroupState::Dropped
                    )
                    && groups.get(g).state == GroupState::Unreleased
                    && !groups.get(g).announced
                {
                    groups.get_mut(g).state = GroupState::Dropped;
                }
            }
            let all_dead = parked.groups.iter().all(|&g| groups.is_dead(g));
            let ready = parked.groups.iter().all(|&g| {
                let after_done = match groups.get(g).kind {
                    GroupKind::Defer {
                        after: Some(after), ..
                    } => groups.get(after).state == GroupState::Completed,
                    _ => true,
                };
                groups.is_dead(g) || (groups.is_released(g) && after_done)
            });
            (all_dead, ready)
        };
        if all_dead {
            // Abandoned deferred work: make its groups terminal so they reclaim.
            let mut groups = shared.groups.lock().unwrap();
            for &g in &parked.groups {
                if groups.get(g).state == GroupState::Unreleased {
                    groups.get_mut(g).state = GroupState::Dropped;
                }
            }
            drop(groups);
            parked.dropped = true;
            parked.start = None;
            continue;
        }
        if ready {
            let start = parked.start.take().expect("starter");
            let futures = start();
            parked.scope = Some(Box::new(Scope::new(
                scope.meta,
                scope.shared,
                parked.set,
                parked.groups.clone(),
                futures,
                Vec::new(),
            )));
            *changed = true;
        }
    }
}

pub(crate) async fn run_loop<'a>(
    root: &mut Scope<'a>,
    shared: &Shared,
    sink: &mut (dyn for<'p> FnMut(Payload<'p>) + Send),
) {
    let mut state = Loop {
        generation: 0,
        initial_shipped: false,
        done: false,
    };
    loop {
        let progress = futures::future::poll_fn(|cx| match root.poll_generation(cx) {
            Poll::Ready(true) => Poll::Ready(true),
            Poll::Ready(false) => {
                if root.has_live_streams() && state.initial_shipped {
                    Poll::Pending
                } else {
                    Poll::Ready(false)
                }
            }
            Poll::Pending if shared.halted.swap(false, Ordering::Relaxed) => Poll::Ready(true),
            Poll::Pending => Poll::Pending,
        })
        .await;
        state.generation += 1;
        let pieces = barrier(root, shared, &mut state);
        if let Some(BarrierOutput {
            kind,
            data,
            errors,
            entries,
            has_next,
        }) = pieces
        {
            let payload = Payload {
                root: Some(&*root as &dyn crate::exec::payload::ErasedRoot),
                kind,
                data,
                errors,
                pending: entries.pending,
                incremental: entries.incremental,
                completed: entries.completed,
                has_next,
            };
            sink(payload);
        }
        if state.done {
            break;
        }
        let changed = advance(root, shared);
        if !progress && !changed && !root.has_live_streams() {
            // Nothing can make progress: emit a terminal payload so the client is not left hanging.
            if state.initial_shipped {
                sink(Payload {
                    root: Some(&*root as &dyn crate::exec::payload::ErasedRoot),
                    kind: PayloadKind::Subsequent,
                    data: Data::Absent,
                    errors: Vec::new(),
                    pending: Vec::new(),
                    incremental: Vec::new(),
                    completed: Vec::new(),
                    has_next: Some(false),
                });
            }
            break;
        }
    }
}
