//! The pull side of an execution: the stream that drives it, and the outlet
//! that hands that stream one encoded payload at a time.

use crate::exec::payload::Payload;
use futures::Stream;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::Poll;

/// Where the executor ships payloads. After each one, `run_loop` waits on
/// `poll_taken`, which registers no waker: `pull` takes the payload in the
/// same poll that shipped it, and the consumer's next poll resumes.
pub(crate) trait Ship: Send {
    fn ship(&mut self, payload: Payload<'_>);
    fn poll_taken(&mut self) -> Poll<()>;
}

pub(crate) struct Outlet<T, E> {
    slot: Arc<Mutex<Option<T>>>,
    encode: E,
}

impl<T, E> Ship for Outlet<T, E>
where
    T: Send,
    E: for<'p> FnMut(Payload<'p>) -> T + Send,
{
    fn ship(&mut self, payload: Payload<'_>) {
        let item = (self.encode)(payload);
        let untaken = self.slot.lock().unwrap().replace(item);
        debug_assert!(untaken.is_none(), "shipped over an untaken payload");
    }

    fn poll_taken(&mut self) -> Poll<()> {
        if self.slot.lock().unwrap().is_none() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Drives `run` from the returned stream: each poll resumes the execution
/// until it ships a payload or waits on a resolver, so nothing runs between
/// polls, and dropping the stream drops the execution.
pub(crate) fn pull<T, E, F, Fut>(encode: E, run: F) -> impl Stream<Item = T>
where
    T: Send,
    E: for<'p> FnMut(Payload<'p>) -> T + Send,
    F: FnOnce(Outlet<T, E>) -> Fut,
    Fut: Future<Output = ()>,
{
    let slot = Arc::new(Mutex::new(None));
    let mut execution = Some(Box::pin(run(Outlet {
        slot: slot.clone(),
        encode,
    })));
    futures::stream::poll_fn(move |cx| {
        if let Some(future) = &mut execution
            && future.as_mut().poll(cx).is_ready()
        {
            execution = None;
        }
        match slot.lock().unwrap().take() {
            Some(item) => Poll::Ready(Some(item)),
            None if execution.is_none() => Poll::Ready(None),
            None => Poll::Pending,
        }
    })
}
