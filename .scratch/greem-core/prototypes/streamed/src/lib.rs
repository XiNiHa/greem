//! THROWAWAY: the barrier/serialization seam, not a GraphQL executor.
#![forbid(unsafe_code)]
pub mod borrowed;
pub mod contract;
use futures::{future::poll_fn, task::AtomicWaker};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
    task::Poll,
};

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Resolve,
    Serialize,
}

struct State {
    epoch: usize,
    phase: Phase,
    next_id: usize,
    // id -> (arrived, serialized). A participant is one live chain.
    participants: BTreeMap<usize, (bool, bool, Arc<AtomicWaker>)>,
    fragments: BTreeMap<usize, String>,
}

#[derive(Clone)]
pub struct Barrier {
    state: Arc<Mutex<State>>,
    wake: Arc<AtomicWaker>,
}

impl Default for Barrier {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                epoch: 0,
                phase: Phase::Resolve,
                next_id: 0,
                participants: BTreeMap::new(),
                fragments: BTreeMap::new(),
            })),
            wake: Arc::new(AtomicWaker::new()),
        }
    }
}

impl Barrier {
    pub fn register(&self) -> Participant {
        let mut state = self.state.lock().unwrap();
        assert!(state.phase == Phase::Resolve);
        let id = state.next_id;
        state.next_id += 1;
        let wake = Arc::new(AtomicWaker::new());
        state.participants.insert(id, (false, false, wake.clone()));
        Participant {
            id,
            barrier: self.clone(),
            wake,
        }
    }

    /// Participants keep their own storage and serialize only when invited.
    /// The sink sees one owned JSON envelope per barrier, never frame borrows.
    pub async fn drive<F: Future + Send>(
        &self,
        root: F,
        mut sink: impl FnMut(String) + Send,
    ) -> F::Output {
        let mut root = Box::pin(root);
        poll_fn(|cx| {
            self.wake.register(cx.waker());
            if let Poll::Ready(result) = root.as_mut().poll(cx) {
                assert!(self.state.lock().unwrap().participants.is_empty());
                return Poll::Ready(result);
            }
            let mut state = self.state.lock().unwrap();
            if state.participants.is_empty() {
                return Poll::Pending;
            }
            match state.phase {
                Phase::Resolve if state.participants.values().all(|p| p.0) => {
                    state.phase = Phase::Serialize;
                }
                Phase::Serialize if state.participants.values().all(|p| p.1) => {
                    let fragments = std::mem::take(&mut state.fragments);
                    let payload = format!(
                        "{{\"barrier\":{},\"fragments\":[{}]}}",
                        state.epoch,
                        fragments.into_values().collect::<Vec<_>>().join(",")
                    );
                    // Unlock around the user callback. Owners are still suspended.
                    drop(state);
                    sink(payload);
                    state = self.state.lock().unwrap();
                    state.epoch += 1;
                    state.phase = Phase::Resolve;
                    for participant in state.participants.values_mut() {
                        participant.0 = false;
                        participant.1 = false;
                    }
                }
                _ => return Poll::Pending,
            }
            let wakes: Vec<_> = state.participants.values().map(|p| p.2.clone()).collect();
            drop(state);
            for wake in wakes {
                wake.wake();
            }
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await
    }
}

pub struct Participant {
    id: usize,
    barrier: Barrier,
    wake: Arc<AtomicWaker>,
}

impl Participant {
    pub async fn publish<T: Serialize + Sync>(&self, borrowed: &T) {
        let epoch = self.barrier.state.lock().unwrap().epoch;
        poll_fn(|cx| {
            self.wake.register(cx.waker());
            let mut state = self.barrier.state.lock().unwrap();
            if state.epoch != epoch {
                return Poll::Ready(());
            }
            let arrived = state.participants[&self.id].0;
            state.participants.get_mut(&self.id).unwrap().0 = true;
            let mut changed = !arrived;
            if state.phase == Phase::Serialize && !state.participants[&self.id].1 {
                // The borrow is local to this future; only serialized bytes escape.
                state
                    .fragments
                    .insert(self.id, serde_json::to_string(borrowed).unwrap());
                state.participants.get_mut(&self.id).unwrap().1 = true;
                changed = true;
            }
            if changed {
                self.barrier.wake.wake();
            }
            Poll::Pending
        })
        .await
    }
}

impl Drop for Participant {
    fn drop(&mut self) {
        self.barrier
            .state
            .lock()
            .unwrap()
            .participants
            .remove(&self.id);
        self.barrier.wake.wake();
    }
}
