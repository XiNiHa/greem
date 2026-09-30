//! THROWAWAY downstream schema/application for the borrowed-payload alternative.
#![forbid(unsafe_code)]
use futures::{
    future::{BoxFuture, poll_fn},
    stream::Stream,
    task::noop_waker,
};
use greem_streamed_probe::borrowed::*;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

struct Parents {
    names: Vec<String>,
    count: usize,
    initial_count: usize,
    capacity: usize,
    log: Log,
    gate: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for Parents {
    fn drop(&mut self) {
        self.log.lock().unwrap().log.push("parents".into());
    }
}

struct Item<'a> {
    parent: &'a str,
    group: usize,
    index: usize,
    value: String,
    // Deliberately invariant: this is not a covariance-only workaround.
    invariant: Mutex<&'a str>,
    log: Log,
}
impl Drop for Item<'_> {
    fn drop(&mut self) {
        assert_eq!(*self.invariant.lock().unwrap(), self.parent);
        self.log
            .lock()
            .unwrap()
            .log
            .push(format!("item:{}:{}", self.group, self.index));
    }
}

struct Source<'a> {
    parent: &'a str,
    group: usize,
    index: usize,
    count: usize,
    delayed: bool,
    log: Log,
}
impl<'a> Stream for Source<'a> {
    type Item = Item<'a>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.log.lock().unwrap().polls += 1;
        if self.index == self.count {
            return Poll::Ready(None);
        }
        if self.group == 1 && !self.delayed {
            self.delayed = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.delayed = false;
        let index = self.index;
        self.index += 1;
        Poll::Ready(Some(Item {
            parent: self.parent,
            group: self.group,
            index,
            value: format!("{}:{index}", self.parent),
            invariant: Mutex::new(self.parent),
            log: self.log.clone(),
        }))
    }
}
impl Drop for Source<'_> {
    fn drop(&mut self) {
        assert!(!self.parent.is_empty());
        self.log
            .lock()
            .unwrap()
            .log
            .push(format!("source:{}", self.group));
    }
}

struct Turn<'a> {
    items: Vec<Item<'a>>,
    // Proves storage batches can be Send without being Sync.
    projections: Cell<usize>,
    gate: Arc<std::sync::atomic::AtomicBool>,
    log: Log,
}
impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut log = self.log.lock().unwrap();
        log.live_turns -= 1;
        log.log.push("turn".into());
    }
}

struct BorrowedChild<'view, 'parent> {
    item: &'view Item<'parent>,
    deferred: bool,
}
impl Drop for BorrowedChild<'_, '_> {
    fn drop(&mut self) {
        self.item.log.lock().unwrap().log.push(format!(
            "child:{}:{}:{}",
            self.item.group, self.item.index, self.deferred
        ));
    }
}

struct TurnWork<'view, 'parent> {
    items: Vec<&'view Item<'parent>>,
    // A real pending async resolver borrows the batch owned by its frame.
    pending: Option<BoxFuture<'view, Vec<BorrowedChild<'view, 'parent>>>>,
    children: Vec<BorrowedChild<'view, 'parent>>,
    deferred: bool,
    gate: Arc<std::sync::atomic::AtomicBool>,
}
impl Batch for Turn<'_> {
    fn start(&self) -> Box<dyn Chain + '_> {
        self.projections.set(self.projections.get() + 1);
        // Reborrow each item at the call lifetime; no covariance over the
        // invariant Item lifetime is required (the full type stays erased).
        Box::new(TurnWork {
            items: self.items.iter().collect(),
            pending: None,
            children: Vec::new(),
            deferred: false,
            gate: self.gate.clone(),
        })
    }
}
impl Chain for TurnWork<'_, '_> {
    fn poll_generation(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.children.is_empty() {
            return Poll::Ready(());
        }
        if self.pending.is_none() {
            let items = self.items.clone(); // References only, never leaf values.
            let gate = self.gate.clone();
            let deferred = self.deferred;
            self.pending = Some(Box::pin(async move {
                let children = items
                    .into_iter()
                    .map(|item| BorrowedChild { item, deferred })
                    .collect();
                let mut yielded = false;
                poll_fn(|cx| {
                    if !gate.load(std::sync::atomic::Ordering::SeqCst) {
                        return Poll::Pending;
                    }
                    if !yielded {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    Poll::Ready(())
                })
                .await;
                children
            }));
        }
        match self.pending.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(children) => {
                self.pending = None;
                self.children = children;
                Poll::Ready(())
            }
        }
    }
    fn records<'view>(&'view self, output: &mut Vec<Record<'view>>) {
        output.extend(self.children.iter().map(|child| Record {
            group: child.item.group,
            index: child.item.index,
            parent: child.item.parent,
            value: &child.item.value,
            deferred: child.deferred,
        }));
    }
    fn advance(&mut self) -> bool {
        if self.deferred {
            return true;
        }
        self.children.clear();
        self.deferred = true;
        false
    }
    fn frame_depth(&self) -> usize {
        0
    }
}

struct Driver<'a> {
    sources: Vec<Option<Pin<Box<Source<'a>>>>>,
    initial: bool,
    initial_count: usize,
    initial_pulled: Vec<usize>,
    admitted: bool,
    capacity: usize,
    next_source: usize,
    buffered: Vec<Item<'a>>,
    turns: Vec<Frame<'a>>,
    gate: Arc<std::sync::atomic::AtomicBool>,
    log: Log,
}
impl Batch for Parents {
    fn start(&self) -> Box<dyn Chain + '_> {
        Box::new(Driver {
            sources: self
                .names
                .iter()
                .enumerate()
                .map(|(group, parent)| {
                    Some(Box::pin(Source {
                        parent,
                        group,
                        index: 0,
                        count: self.count,
                        delayed: false,
                        log: self.log.clone(),
                    }))
                })
                .collect(),
            initial: true,
            initial_count: self.initial_count,
            initial_pulled: vec![0; self.names.len()],
            admitted: false,
            capacity: self.capacity,
            next_source: 0,
            buffered: Vec::new(),
            turns: Vec::new(),
            gate: self.gate.clone(),
            log: self.log.clone(),
        })
    }
}
impl Chain for Driver<'_> {
    fn poll_generation(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        // Pump on every poll, including while a child's resolver is Pending.
        // Capacity applies to the continuation buffer, not the exact initial set.
        let start = self.next_source;
        let source_count = self.sources.len();
        for offset in 0..source_count {
            let group = (start + offset) % source_count;
            let source = &mut self.sources[group];
            while source.is_some()
                && if self.initial {
                    self.initial_pulled[group] < self.initial_count
                } else {
                    self.buffered.len() < self.capacity
                }
            {
                match source.as_mut().unwrap().as_mut().poll_next(cx) {
                    Poll::Ready(Some(item)) => {
                        self.initial_pulled[group] += usize::from(self.initial);
                        self.buffered.push(item);
                        self.next_source = (group + 1) % source_count;
                    }
                    Poll::Ready(None) => {
                        *source = None;
                    }
                    Poll::Pending => break,
                }
            }
        }
        let initial_ready =
            !self.initial
                || self.sources.iter().enumerate().all(|(group, s)| {
                    s.is_none() || self.initial_pulled[group] == self.initial_count
                });
        if initial_ready && !self.admitted && !self.buffered.is_empty() {
            let items = std::mem::take(&mut self.buffered);
            {
                let mut log = self.log.lock().unwrap();
                log.live_turns += 1;
                log.max_live_turns = log.max_live_turns.max(log.live_turns);
            }
            self.turns.push(Frame::from_batch(Box::new(Turn {
                items,
                projections: Cell::new(0),
                gate: self.gate.clone(),
                log: self.log.clone(),
            })));
            self.admitted = true;
        }
        {
            let mut log = self.log.lock().unwrap();
            log.max_depth = log.max_depth.max(1 + self.frame_depth());
        }
        let mut ready = initial_ready;
        for turn in &mut self.turns {
            if turn.poll_generation(cx).is_pending() {
                ready = false;
            }
        }
        if ready
            && (self.initial && self.initial_count == 0
                || !self.turns.is_empty()
                || self.sources.iter().all(Option::is_none))
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    fn records<'view>(&'view self, output: &mut Vec<Record<'view>>) {
        for turn in &self.turns {
            turn.records(output);
        }
        // Several independently owned frames contribute ordinary borrowed fields
        // to one flat payload. This method does no serialization.
    }
    fn advance(&mut self) -> bool {
        self.turns.retain_mut(|turn| !turn.advance());
        self.admitted = false;
        self.initial = false;
        self.turns.is_empty()
            && self.buffered.is_empty()
            && self.sources.iter().all(Option::is_none)
    }
    fn frame_depth(&self) -> usize {
        self.turns.iter().map(Chain::frame_depth).max().unwrap_or(0)
    }
}

fn main() {
    #[cfg(escape)]
    {
        let mut escaped: Option<&str> = None;
        let root = Frame::from_batch(Box::new(Parents {
            names: vec!["Ada".into()],
            count: 1,
            initial_count: 1,
            capacity: 1,
            log: Arc::default(),
            gate: Arc::new(true.into()),
        }));
        futures::executor::block_on(execute(root, |payload| {
            escaped = Some(payload.records[0].value);
        }));
        println!("{escaped:?}");
    }
    #[cfg(not(escape))]
    for (count, initial_count, cancel, panic_sink) in [
        (3, 1, 0, false),
        (1_000, 1, 0, false),
        (3, 0, 0, false),
        (3, 8, 0, false),
        (3, 1, 1, false),
        (3, 1, 2, false),
        (3, 1, 0, true),
    ] {
        let log: Log = Arc::default();
        let gate: Arc<std::sync::atomic::AtomicBool> = Arc::new((cancel != 1).into());
        let root = Frame::from_batch(Box::new(Parents {
            names: vec!["Ada".into(), "Lin".into()],
            count,
            initial_count,
            capacity: 1,
            log: log.clone(),
            gate: gate.clone(),
        }));
        let mut seen = std::collections::BTreeSet::new();
        let mut bytes_count = 0;
        let mut sink_calls = 0;
        let future = execute(root, |payload| {
            assert!(!log.lock().unwrap().log.iter().any(|e| e == "parents"));
            // Inspect actual fields before encoding. A bytes facade cannot do this.
            for record in &payload.records {
                assert_eq!(record.parent, if record.group == 0 { "Ada" } else { "Lin" });
                assert!(
                    !log.lock()
                        .unwrap()
                        .log
                        .contains(&format!("item:{}:{}", record.group, record.index))
                );
                assert!(seen.insert((record.group, record.index, record.deferred)));
            }
            if payload.barrier == 0 {
                assert_eq!(payload.records.len(), 2 * initial_count.min(count));
                assert!(
                    payload
                        .records
                        .iter()
                        .all(|r| r.index < initial_count && !r.deferred)
                );
            }
            if panic_sink {
                panic!("intentional sink panic");
            }
            let first = serde_json::to_string(&payload).unwrap();
            let second = serde_json::to_string(&payload).unwrap();
            assert_eq!(
                first, second,
                "serializing the view must not advance chains"
            );
            bytes_count += first.len();
            sink_calls += 1;
            if cancel == 2 && sink_calls == 1 {
                gate.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            if count == 3 {
                println!("{first}");
            }
        });
        // Enforce Send on the fully composed execution future.
        let mut future: Pin<Box<dyn Future<Output = ()> + Send + '_>> = Box::pin(future);
        if cancel != 0 {
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);
            for _ in 0..8 {
                assert!(future.as_mut().poll(&mut cx).is_pending());
            }
            drop(future);
            assert_eq!(sink_calls, usize::from(cancel == 2));
        } else if panic_sink {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                futures::executor::block_on(future);
            }));
            assert!(result.is_err());
        } else {
            futures::executor::block_on(future);
            assert_eq!(seen.len(), count * 2 * 2);
        }
        let evidence = log.lock().unwrap();
        assert_eq!(evidence.live_turns, 0);
        assert_eq!(evidence.log.last().map(String::as_str), Some("parents"));
        for (position, event) in evidence.log.iter().enumerate() {
            if let Some(suffix) = event.strip_prefix("child:") {
                let (item, _) = suffix.rsplit_once(':').unwrap();
                let owner = evidence
                    .log
                    .iter()
                    .position(|e| e == &format!("item:{item}"))
                    .unwrap();
                assert!(position < owner);
            }
        }
        assert!(evidence.max_live_turns <= 2);
        assert_eq!(evidence.max_depth, 2);
        println!(
            "count={count} cancel={cancel} panic={panic_sink}: sink_calls={sink_calls} bytes={bytes_count} max_live_turns={} frame_depth={} destruction=ok",
            evidence.max_live_turns, evidence.max_depth
        );
    }
}
