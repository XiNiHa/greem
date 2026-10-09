//! The generation loop: poll the whole tree one generation, run the barrier
//! (null pass, payload, release), advance, repeat.

use crate::exec::barrier::{barrier, stalled};
use crate::exec::payload::Step;
use crate::exec::pull::Ship;
use crate::exec::scope::{Activity, DeferredSetState, FieldState, POLL, STREAM, Scope};
use crate::exec::state::{GroupKind, GroupState, Shared};
use std::sync::Arc;
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
    // Retired turns and finished scopes dropped their group references above;
    // terminal groups nobody references any more give their slots back.
    shared.groups().sweep();
    changed
}

/// Advances one scope's subtree; a subtree that changed nothing and has no
/// work left becomes quiescent, so later polls and barriers skip it.
fn advance_scope(scope: &mut Scope<'_>, shared: &Shared, changed: &mut bool) {
    // Quiescent is final: its holds went when it became so.
    if scope.activity == Activity::Quiescent {
        return;
    }
    // Every object here belongs to a dead group: nothing it produces can be
    // delivered, so its pending futures are never polled again.
    if scope.all_dead(&shared.groups()) {
        scope.activity = Activity::Quiescent;
        scope.release_holds(&mut shared.groups());
        return;
    }
    let mut local = false;
    advance_scope_inner(scope, shared, &mut local);
    if local {
        *changed = true;
    } else if scope.is_finished(&shared.groups()) {
        scope.activity = Activity::Quiescent;
        scope.release_holds(&mut shared.groups());
    }
}

fn advance_scope_inner(scope: &mut Scope<'_>, shared: &Shared, changed: &mut bool) {
    if scope.activity == Activity::Quiescent {
        return;
    }
    if scope.activity == Activity::Fresh {
        // Created in the generation that just ended; polled from the next.
        scope.activity = Activity::Active;
        scope.signal.raise(POLL);
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
                if let Some(turn) = driver.make_turn(column) {
                    *changed = true;
                    scope.signal.raise(STREAM);
                    // Made between generations: the next one polls its scopes.
                    column.turns[turn].each_child_mut(|s| {
                        s.activity = Activity::Active;
                        s.signal.raise(POLL);
                    });
                }
                // A released stream is pumped every generation: its sources
                // may have stopped at a full buffer rather than at a wake.
                if !driver.is_done() {
                    scope.signal.raise(POLL);
                }
            }
            column.stream = Some(driver);
        }
    }
    for (d, deferred) in scope.deferred.iter_mut().enumerate() {
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
        if all_dead || deferred.excluded.iter().all(|&x| x) {
            deferred.state = DeferredSetState::Dropped;
            deferred.release_hold(&mut shared.groups());
            continue;
        }
        if ready {
            let DeferredSetState::Waiting(start) =
                std::mem::replace(&mut deferred.state, DeferredSetState::Dropped)
            else {
                unreachable!()
            };
            let futures = start();
            let at: Arc<[Step]> = scope
                .path
                .iter()
                .copied()
                .chain(std::iter::once(Step::Deferred(d as u32)))
                .collect();
            {
                let mut groups = shared.groups();
                deferred.release_hold(&mut groups);
                for (o, g) in deferred.live_objects() {
                    groups.register_root(g, at.clone(), o);
                }
            }
            let mut inner = Scope::new(
                scope.meta,
                scope.shared,
                at,
                deferred.signal.clone(),
                deferred.set,
                deferred.groups.clone(),
                futures,
                Vec::new(),
            );
            // Started between generations: the next one polls it.
            inner.activity = Activity::Active;
            inner.signal.raise(POLL);
            deferred.state = DeferredSetState::Running(Box::new(inner));
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
                if shared.live_streams.load(Ordering::Relaxed) > 0 && state.initial_shipped {
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
        let stuck = !progress && !changed && shared.live_streams.load(Ordering::Relaxed) == 0;
        debug_assert!(!stuck, "execution stalled: nothing can make progress");
        if stuck {
            // End the response anyway so the client is not left hanging.
            let output = stalled(&mut shared.groups(), state.initial_shipped);
            sink.ship(output.into_payload(&*root));
            break;
        }
    }
}
