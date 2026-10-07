//! The generation loop: poll the whole tree one generation, run the barrier
//! (null pass, payload, release), advance, repeat.

use crate::exec::barrier::{BarrierOutput, barrier};
use crate::exec::payload::{Data, Payload, PayloadKind};
use crate::exec::scope::{Activity, DeferredSetState, FieldState, Scope};
use crate::exec::state::{GroupId, GroupKind, GroupState, Shared};
use std::sync::atomic::Ordering;
use std::task::Poll;

pub(crate) struct Loop {
    pub generation: u32,
    pub initial_shipped: bool,
    pub done: bool,
}

/// After the sink returns: release announced groups, start deferred sets,
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
    if scope.activity != Activity::Quiescent && scope.all_dead(shared) {
        scope.activity = Activity::Quiescent;
        return;
    }
    let mut local = false;
    advance_scope_inner(scope, shared, &mut local);
    if local {
        *changed = true;
    } else if scope.is_finished() {
        scope.activity = Activity::Quiescent;
    }
}

fn advance_scope_inner(scope: &mut Scope<'_>, shared: &Shared, changed: &mut bool) {
    if scope.activity == Activity::Quiescent {
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
    for deferred in &mut scope.deferred {
        match &mut deferred.state {
            DeferredSetState::Waiting(_) => {}
            DeferredSetState::Running(inner) => {
                advance_scope(inner, shared, changed);
                continue;
            }
            DeferredSetState::Dropped => continue,
        }
        let (all_dead, ready) = {
            let mut groups = shared.groups.lock().unwrap();
            // A group whose `after` dependency failed or was dropped is
            // dropped too; one already announced was failed by the barrier.
            for &g in &deferred.groups {
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
            let all_dead = deferred.groups.iter().all(|&g| groups.is_dead(g));
            let ready = deferred.groups.iter().all(|&g| {
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
            for &g in &deferred.groups {
                if groups.get(g).state == GroupState::Unreleased {
                    groups.get_mut(g).state = GroupState::Dropped;
                }
            }
            drop(groups);
            deferred.state = DeferredSetState::Dropped;
            continue;
        }
        if ready {
            let DeferredSetState::Waiting(start) =
                std::mem::replace(&mut deferred.state, DeferredSetState::Dropped)
            else {
                unreachable!()
            };
            let futures = start();
            deferred.state = DeferredSetState::Running(Box::new(Scope::new(
                scope.meta,
                scope.shared,
                deferred.set,
                deferred.groups.clone(),
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
