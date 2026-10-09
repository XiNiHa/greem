//! The generation loop: poll the whole tree one generation, run the barrier
//! (null pass, payload, release), advance, repeat.

use crate::exec::barrier::{barrier, stalled};
use crate::exec::pull::Ship;
use crate::exec::scope::{Activity, DeferredSetState, FieldState, Scope};
use crate::exec::state::{GroupKind, GroupState, Shared};
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
        let mut groups = shared.groups();
        for g in groups.take_announced() {
            if !matches!(groups.get(g).state, GroupState::Announced) {
                continue;
            }
            groups.set_state(g, GroupState::Released);
            // The next barrier can ship it, even when it starts no work
            // itself (a fragment whose only content is a nested defer).
            changed = true;
            if groups.is_dead(g) {
                continue;
            }
            // A shared field set runs as soon as one member fragment may.
            for s in groups.sharing(g) {
                if matches!(groups.get(s).state, GroupState::Unreleased) {
                    groups.set_state(s, GroupState::Released);
                }
            }
        }
    }
    advance_scope(root, shared, &mut changed);
    root.clear_fresh();
    // Retired turns and finished scopes dropped their group references above;
    // terminal groups nobody references any more give their slots back.
    shared.groups().sweep();
    changed
}

/// Advances one scope's subtree; a subtree that changed nothing and has no
/// work left becomes quiescent, so later polls and barriers skip it.
fn advance_scope(scope: &mut Scope<'_>, shared: &Shared, changed: &mut bool) {
    // Every object here belongs to a dead group: nothing it produces can be
    // delivered, so its pending futures are never polled again.
    if scope.activity != Activity::Quiescent && scope.all_dead(&shared.groups()) {
        scope.activity = Activity::Quiescent;
        return;
    }
    let mut local = false;
    advance_scope_inner(scope, shared, &mut local);
    if local {
        *changed = true;
    } else if scope.is_finished(&shared.groups()) {
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
            turn.each_child_mut(|s| advance_scope(s, shared, changed));
        }
        for turn in column.turns.iter_mut().skip(1) {
            if turn.shipped && !turn.retired && {
                let groups = shared.groups();
                !turn.any_child(|s| !s.is_finished(&groups))
            } {
                // Retiring drops scopes, which lock the table themselves.
                turn.retire();
            }
        }
        if let Some(mut driver) = column.stream.take() {
            let released = driver.is_released() || {
                let groups = shared.groups();
                driver
                    .groups()
                    .all(|g| groups.is_released(g) || groups.is_dead(g))
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
            let mut groups = shared.groups();
            // A group whose `after` dependency failed or was dropped is
            // dropped too; one already announced was failed by the barrier.
            for &g in &deferred.groups {
                if let GroupKind::Defer {
                    after: Some(after), ..
                } = groups.get(g).kind
                    && matches!(
                        groups.get(after).state,
                        GroupState::Failed(_) | GroupState::Dropped
                    )
                    && matches!(groups.get(g).state, GroupState::Unreleased)
                {
                    groups.set_state(g, GroupState::Dropped);
                }
            }
            let all_dead = deferred.groups.iter().all(|&g| groups.is_dead(g));
            if all_dead {
                // Abandoned deferred work: make its groups terminal so they reclaim.
                for &g in &deferred.groups {
                    if matches!(groups.get(g).state, GroupState::Unreleased) {
                        groups.set_state(g, GroupState::Dropped);
                    }
                }
            }
            let ready = !all_dead
                && deferred.groups.iter().all(|&g| {
                    let after_done = match groups.get(g).kind {
                        GroupKind::Defer {
                            after: Some(after), ..
                        } => matches!(groups.get(after).state, GroupState::Completed),
                        _ => true,
                    };
                    groups.is_dead(g) || (groups.is_released(g) && after_done)
                });
            (all_dead, ready)
        };
        if all_dead {
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

pub(crate) async fn run_loop<'a>(root: &mut Scope<'a>, shared: &Shared, sink: &mut dyn Ship) {
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
        if let Some(output) = barrier(root, shared, &mut state) {
            sink.ship(output.into_payload(&*root));
        }
        if state.done {
            break;
        }
        // Nothing runs until the consumer takes the payload.
        futures::future::poll_fn(|_| sink.poll_taken()).await;
        let changed = advance(root, shared);
        let stuck = !progress && !changed && !root.has_live_streams();
        debug_assert!(!stuck, "execution stalled: nothing can make progress");
        if stuck {
            // End the response anyway so the client is not left hanging.
            let output = stalled(&mut shared.groups(), state.initial_shipped);
            sink.ship(output.into_payload(&*root));
            break;
        }
    }
}
