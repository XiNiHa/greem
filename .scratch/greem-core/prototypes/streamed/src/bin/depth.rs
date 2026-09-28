//! THROWAWAY: actual nested owner/dependent frames on a 2 MiB debug stack.
#![forbid(unsafe_code)]
use futures::{StreamExt, future::poll_fn, stream::BoxStream, task::noop_waker};
use greem_streamed_probe::{
    borrowed::{self, Batch, Chain, Frame, Record},
    contract::*,
};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context as TaskContext, Poll},
};

struct State {
    depth: usize,
    count: usize,
    hold: bool,
    panic: bool,
    deepest: AtomicBool,
    drops: Mutex<Vec<(usize, usize)>>,
    max_depth: Mutex<usize>,
    started_roots: Mutex<Vec<usize>>,
}
struct Root {
    seed: String,
}
struct DeepValue<'a> {
    parent: &'a str,
    value: String,
    remaining: usize,
    id: usize,
    state: &'a State,
}
impl Drop for DeepValue<'_> {
    fn drop(&mut self) {
        assert!(!self.parent.is_empty());
        self.state
            .drops
            .lock()
            .unwrap()
            .push((self.id, self.remaining));
    }
}
struct DeepTag;
struct DeepItems;
impl Field for DeepItems {
    type Type = List<DeepTag>;
    type Args = ();
}
impl Resolver<DeepItems, State> for Root {
    type Output<'a>
        = Streamed<BoxStream<'a, Result<DeepValue<'a>, Error>>>
    where
        Self: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<State>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents
            .iter()
            .map(|parent| {
                let parent = *parent;
                Streamed::new(
                    futures::stream::unfold(0, move |id| async move {
                        if id == ctx.0.count {
                            return None;
                        }
                        Some((
                            Ok(DeepValue {
                                parent: &parent.seed,
                                value: format!("root:{id}"),
                                remaining: ctx.0.depth - 2,
                                id,
                                state: &ctx.0,
                            }),
                            id + 1,
                        ))
                    })
                    .boxed(),
                )
            })
            .collect())
    }
}
struct DeepBatch<'a> {
    values: Vec<DeepValue<'a>>,
}
struct Leaf<'view, 'owner> {
    values: Vec<&'view DeepValue<'owner>>,
    ready: bool,
}
impl Drop for Leaf<'_, '_> {
    fn drop(&mut self) {
        for value in &self.values {
            assert!(!value.value.is_empty());
        }
    }
}
impl Chain for Leaf<'_, '_> {
    fn poll_generation(&mut self, _: &mut TaskContext<'_>) -> Poll<()> {
        for value in &self.values {
            value.state.deepest.store(true, Ordering::SeqCst);
            if value.state.panic {
                panic!("intentional deepest resolver panic");
            }
            if value.state.hold {
                return Poll::Pending;
            }
        }
        self.ready = true;
        Poll::Ready(())
    }
    fn records<'a>(&'a self, out: &mut Vec<Record<'a>>) {
        if self.ready {
            out.extend(self.values.iter().map(|v| Record {
                group: 0,
                index: v.id,
                parent: v.parent,
                value: &v.value,
                deferred: false,
            }));
        }
    }
    fn advance(&mut self) -> bool {
        true
    }
    fn frame_depth(&self) -> usize {
        0
    }
}
impl Batch for DeepBatch<'_> {
    fn start(&self) -> Box<dyn Chain + '_> {
        assert!(!self.values.is_empty());
        let values: Vec<_> = self.values.iter().collect();
        if self.values[0].remaining == 0 {
            return Box::new(Leaf {
                values,
                ready: false,
            });
        }
        Box::new(Resolution {
            pending: Some(Box::pin(async move {
                let mut yielded = false;
                poll_fn(|cx| {
                    if yielded {
                        Poll::Ready(())
                    } else {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await;
                let children = values
                    .into_iter()
                    .map(|p| DeepValue {
                        parent: &p.value,
                        value: format!("{}:child", p.value),
                        remaining: p.remaining - 1,
                        id: p.id,
                        state: p.state,
                    })
                    .collect();
                Box::new(Frame::from_batch(Box::new(DeepBatch { values: children })))
                    as Box<dyn Chain>
            })),
            child: None,
            created: false,
        })
    }
}
impl<'owner> Completes<DeepValue<'owner>, State> for DeepTag {
    fn complete<'a>(
        values: Vec<DeepValue<'owner>>,
        _: Vec<Position>,
        _: &'a Context<State>,
        _: Options,
    ) -> Box<dyn Chain + 'a>
    where
        DeepValue<'owner>: 'a,
    {
        Box::new(Frame::from_batch(Box::new(DeepBatch { values })))
    }
}

struct MutationBatch<'a> {
    roots: Vec<Root>,
    ctx: &'a Context<State>,
}
struct MutationWork<'a> {
    roots: Vec<&'a Root>,
    ctx: &'a Context<State>,
    next: usize,
    active: Option<Frame<'a>>,
    parked: Vec<Frame<'a>>,
    emit: bool,
    parked_depth: usize,
}
impl Batch for MutationBatch<'_> {
    fn start(&self) -> Box<dyn Chain + '_> {
        Box::new(MutationWork {
            roots: self.roots.iter().collect(),
            ctx: self.ctx,
            next: 0,
            active: None,
            parked: Vec::new(),
            emit: false,
            parked_depth: 0,
        })
    }
}
impl Chain for MutationWork<'_> {
    fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<()> {
        if self.emit {
            return Poll::Ready(());
        }
        if self.active.is_none() {
            self.ctx.0.started_roots.lock().unwrap().push(self.next);
            self.active = Some(Frame::from_batch(Box::new(DeepBatch {
                values: vec![DeepValue {
                    parent: &self.roots[self.next].seed,
                    value: format!("mutation:{}", self.next),
                    remaining: self.ctx.0.depth - 2,
                    id: self.next,
                    state: &self.ctx.0,
                }],
            })));
        }
        let depth = 1 + self.frame_depth();
        let mut maximum = self.ctx.0.max_depth.lock().unwrap();
        *maximum = (*maximum).max(depth);
        drop(maximum);
        self.active.as_mut().unwrap().poll_generation(cx)
    }
    fn records<'a>(&'a self, out: &mut Vec<Record<'a>>) {
        if self.emit {
            for root in &self.parked {
                root.records(out);
            }
        }
    }
    fn advance(&mut self) -> bool {
        if self.emit {
            return true;
        }
        if self.active.as_mut().unwrap().advance() {
            self.parked_depth = self
                .parked_depth
                .max(self.active.as_ref().unwrap().frame_depth());
            self.parked.push(self.active.take().unwrap());
            self.next += 1;
            self.emit = self.next == self.roots.len();
        }
        false
    }
    fn frame_depth(&self) -> usize {
        self.parked_depth
            .max(self.active.as_ref().map_or(0, Chain::frame_depth))
    }
}

fn main() {
    for (mode, depth, count) in [
        ("normal", 32, 1),
        ("normal", 64, 1),
        ("cancel", 64, 1),
        ("panic", 64, 1),
        ("stream", 64, 1000),
        ("mutation", 64, 1000),
    ] {
        std::thread::Builder::new().name(format!("probe-{mode}"))
            .stack_size(2 * 1024 * 1024).spawn(move || {
                let control = Arc::new(Mutex::new(Control::default()));
                let options = Options { control: control.clone(), initial_count: 1,
                    capacity: 1, incremental: true, defer: false };
                let ctx = Context(State { depth, count,
                    hold: mode == "cancel", panic: mode == "panic", deepest: AtomicBool::new(false),
                    drops: Mutex::new(Vec::new()), max_depth: Mutex::new(0), started_roots: Mutex::new(Vec::new()) });
                let batch: Box<dyn Batch + '_> = if mode == "mutation" {
                    Box::new(MutationBatch { roots: (0..count).map(|_| Root { seed: "root".into() }).collect(), ctx: &ctx })
                } else {
                    Box::new(RootBatch::<_, DeepItems, State> { values: vec![Root { seed: "root".into() }],
                        positions: vec![Position { group: 0, index: 0, deferred: false }], args: (), ctx: &ctx, options })
                };
                let mut seen = Vec::new(); let mut data_payloads = 0;
                let future = borrowed::execute(Frame::from_batch(batch), |payload| {
                    if !payload.records.is_empty() {
                        if mode == "mutation" { assert!(ctx.0.drops.lock().unwrap().is_empty(), "parked roots dropped before serialization"); }
                        for record in &payload.records { seen.push(record.index); }
                        let bytes = serde_json::to_vec(&payload).unwrap();
                        assert!(!bytes.is_empty()); data_payloads += 1;
                    }
                });
                let mut future: Pin<Box<dyn Future<Output = ()> + Send + '_>> = Box::pin(future);
                if mode == "cancel" {
                    let waker = noop_waker(); let mut cx = TaskContext::from_waker(&waker);
                    for _ in 0..10000 {
                        assert!(future.as_mut().poll(&mut cx).is_pending());
                        if ctx.0.deepest.load(Ordering::SeqCst) { break; }
                    }
                    assert!(ctx.0.deepest.load(Ordering::SeqCst)); drop(future);
                } else if mode == "panic" {
                    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| futures::executor::block_on(future))).is_err());
                } else {
                    futures::executor::block_on(future);
                    assert_eq!(seen, (0..count).collect::<Vec<_>>());
                    if mode == "mutation" {
                        assert_eq!(data_payloads, 1);
                        assert_eq!(*ctx.0.started_roots.lock().unwrap(), (0..count).collect::<Vec<_>>());
                    }
                }
                let mut drop_order: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
                for (id, level) in ctx.0.drops.lock().unwrap().iter().copied() {
                    drop_order.entry(id).or_default().push(level);
                }
                assert_eq!(drop_order.len(), count);
                for levels in drop_order.values() { assert_eq!(levels, &(0..depth-1).collect::<Vec<_>>()); }
                let state = control.lock().unwrap();
                let maximum = if mode == "mutation" { *ctx.0.max_depth.lock().unwrap() } else { 1 + state.max_depth };
                // A panic interrupts the poll before its final metrics update.
                if mode != "panic" { assert_eq!(maximum, depth); }
                assert!(state.max_turns <= 2 * depth + 4);
                assert_eq!(state.live_turns, 0);
                println!("PASS {mode}: depth={depth} roots_or_items={count} stack=2097152 data_payloads={data_payloads} max_turns={} every_drop_child_before_parent=true", state.max_turns);
            }).unwrap().join().unwrap();
    }
}
