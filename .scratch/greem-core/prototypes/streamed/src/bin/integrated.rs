//! THROWAWAY generated schema equivalents + application in a downstream crate.
#![forbid(unsafe_code)]
use futures::{
    future::poll_fn,
    stream::{BoxStream, Stream, StreamExt},
    task::noop_waker,
};
use greem_streamed_probe::{
    borrowed::{self, Batch, Chain, Frame, Record},
    contract::*,
};
use std::{
    cell::Cell,
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context as TaskContext, Poll},
};

struct Items;
struct ReadyItems;
struct Name;
struct Details;
struct DetailName;
struct ItemTag;
struct DetailTag;
impl Field for Items {
    type Type = List<ItemTag>;
    type Args = ();
}
impl Field for ReadyItems {
    type Type = List<ItemTag>;
    type Args = ();
}
impl Field for Name {
    type Type = Text;
    type Args = ();
}
impl Field for Details {
    type Type = DetailTag;
    type Args = ();
}
impl Field for DetailName {
    type Type = Text;
    type Args = ();
}

struct App {
    options: Options,
    count: usize,
    source_error: bool,
    halt: bool,
    gate: AtomicBool,
    panic_resolver: bool,
    defer_halt: bool,
}
struct Parent {
    name: String,
    group: usize,
    control: Arc<Mutex<Control>>,
}
impl Drop for Parent {
    fn drop(&mut self) {
        self.control
            .lock()
            .unwrap()
            .drops
            .push(format!("parent:{}", self.group));
    }
}
struct Item<'a> {
    parent: &'a str,
    value: String,
    group: usize,
    index: usize,
    invariant: Mutex<&'a str>,
    control: Arc<Mutex<Control>>,
}
impl Drop for Item<'_> {
    fn drop(&mut self) {
        assert_eq!(*self.invariant.lock().unwrap(), self.parent);
        self.control
            .lock()
            .unwrap()
            .drops
            .push(format!("item:{}:{}", self.group, self.index));
    }
}
struct Detail<'a> {
    owner: &'a str,
    value: String,
    group: usize,
    index: usize,
    control: Arc<Mutex<Control>>,
}
impl Drop for Detail<'_> {
    fn drop(&mut self) {
        assert!(!self.owner.is_empty());
        self.control
            .lock()
            .unwrap()
            .drops
            .push(format!("detail:{}:{}", self.group, self.index));
    }
}
struct Source<'a> {
    parent: &'a Parent,
    ctx: &'a Context<App>,
    index: usize,
    delayed: bool,
}
impl<'a> Stream for Source<'a> {
    type Item = Result<Item<'a>, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.index == self.ctx.0.count {
            return Poll::Ready(None);
        }
        if self.parent.group == 1 && !self.delayed {
            self.delayed = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.delayed = false;
        let index = self.index;
        self.index += 1;
        if self.ctx.0.source_error && self.parent.group == 0 && index == 2 {
            return Poll::Ready(Some(Err(Error("non-null item"))));
        }
        Poll::Ready(Some(Ok(Item {
            parent: &self.parent.name,
            value: format!("{}:{index}", self.parent.name),
            group: self.parent.group,
            index,
            invariant: Mutex::new(&self.parent.name),
            control: self.parent.control.clone(),
        })))
    }
}
impl Drop for Source<'_> {
    fn drop(&mut self) {
        assert!(!self.parent.name.is_empty());
        self.parent
            .control
            .lock()
            .unwrap()
            .drops
            .push(format!("source:{}:{}", self.parent.group, self.index));
    }
}
impl Resolver<Items, App> for Parent {
    type Output<'a>
        = Streamed<BoxStream<'a, Result<Item<'a>, Error>>>
    where
        Self: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.0
            .options
            .control
            .lock()
            .unwrap()
            .calls
            .push(("items", parents.len()));
        Ok(parents
            .iter()
            .map(|p| {
                Streamed::new(
                    Source {
                        parent: p,
                        ctx,
                        index: 0,
                        delayed: false,
                    }
                    .boxed(),
                )
            })
            .collect())
    }
}
impl Resolver<ReadyItems, App> for Parent {
    type Output<'a>
        = Vec<Item<'a>>
    where
        Self: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.0
            .options
            .control
            .lock()
            .unwrap()
            .calls
            .push(("vec-items", parents.len()));
        Ok(parents
            .iter()
            .map(|parent| {
                (0..ctx.0.count)
                    .map(|index| Item {
                        parent: &parent.name,
                        value: format!("{}:{index}", parent.name),
                        group: parent.group,
                        index,
                        invariant: Mutex::new(&parent.name),
                        control: parent.control.clone(),
                    })
                    .collect()
            })
            .collect())
    }
}
#[cfg(not(missing))]
impl Resolver<Name, App> for Item<'_> {
    #[cfg(not(wrong))]
    type Output<'a>
        = Result<&'a str, Error>
    where
        Self: 'a;
    #[cfg(wrong)]
    type Output<'a>
        = i32
    where
        Self: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.0
            .options
            .control
            .lock()
            .unwrap()
            .calls
            .push(("name", parents.len()));
        let mut yielded = 0;
        poll_fn(|cx| {
            if ctx.0.panic_resolver {
                panic!("intentional resolver panic");
            }
            if !ctx.0.gate.load(Ordering::SeqCst) {
                return Poll::Pending;
            }
            if yielded < 2 {
                yielded += 1;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(())
        })
        .await;
        #[cfg(not(wrong))]
        {
            Ok(parents
                .iter()
                .map(|p| {
                    if ctx.0.halt && p.group == 0 && p.index == 1 {
                        Err(Error("HALT"))
                    } else {
                        Ok(p.value.as_str())
                    }
                })
                .collect())
        }
        #[cfg(wrong)]
        {
            Ok(vec![1; parents.len()])
        }
    }
}
impl Resolver<Details, App> for Item<'_> {
    type Output<'a>
        = Detail<'a>
    where
        Self: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.0
            .options
            .control
            .lock()
            .unwrap()
            .calls
            .push(("details", parents.len()));
        Ok(parents
            .iter()
            .map(|p| Detail {
                owner: &p.value,
                value: format!("{}:detail", p.value),
                group: p.group,
                index: p.index,
                control: p.control.clone(),
            })
            .collect())
    }
}
impl Resolver<DetailName, App> for Detail<'_> {
    type Output<'a>
        = Result<&'a str, Error>
    where
        Self: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.0
            .options
            .control
            .lock()
            .unwrap()
            .calls
            .push(("detail-name", parents.len()));
        poll_fn(|_| {
            if ctx.0.gate.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        Ok(parents
            .iter()
            .map(|p| {
                if ctx.0.defer_halt && p.group == 0 && p.index == 1 {
                    Err(Error("deferred HALT"))
                } else {
                    Ok(p.value.as_str())
                }
            })
            .collect())
    }
}

// Generic generated object-tag impls: no application Rust type registration.
struct ItemBatch<'a, T, C> {
    values: Vec<T>,
    positions: Vec<Position>,
    ctx: &'a Context<C>,
    options: Options,
    non_sync: Cell<usize>,
}
impl<C: Send + Sync, T: Resolver<Name, C> + Resolver<Details, C>> Batch for ItemBatch<'_, T, C> {
    fn start(&self) -> Box<dyn Chain + '_> {
        self.non_sync.set(self.non_sync.get() + 1);
        let parents: Vec<_> = self.values.iter().collect();
        let projected = parents.clone();
        let positions = self.positions.clone();
        let ctx = self.ctx;
        let options = self.options.clone();
        Box::new(ItemWork::<T, C> {
            parents,
            positions: self.positions.clone(),
            ctx,
            options: self.options.clone(),
            deferred: false,
            chain: Box::new(Resolution {
                pending: Some(Box::pin(async move {
                    let (projected, positions): (Vec<_>, Vec<_>) = {
                        let control = options.control.lock().unwrap();
                        projected
                            .into_iter()
                            .zip(positions)
                            .filter(|(_, p)| !control.is_failed(p.group))
                            .unzip()
                    };
                    if projected.is_empty() {
                        return <Text as Completes<Result<&str, Error>, C>>::complete(
                            vec![],
                            positions,
                            ctx,
                            options,
                        );
                    }
                    let output = <T as Resolver<Name, C>>::resolve(&projected, &(), ctx)
                        .await
                        .unwrap();
                    <<T as Resolver<Name, C>>::Output<'_> as Outputs<Text, C>>::complete(
                        output, positions, ctx, options,
                    )
                })),
                child: None,
                created: false,
            }),
        })
    }
}
struct ItemWork<'a, T, C> {
    parents: Vec<&'a T>,
    positions: Vec<Position>,
    ctx: &'a Context<C>,
    options: Options,
    deferred: bool,
    chain: Box<dyn Chain + 'a>,
}
impl<C: Send + Sync, T: Resolver<Name, C> + Resolver<Details, C>> Chain for ItemWork<'_, T, C> {
    fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<()> {
        self.chain.poll_generation(cx)
    }
    fn records<'a>(&'a self, output: &mut Vec<Record<'a>>) {
        self.chain.records(output);
    }
    fn advance(&mut self) -> bool {
        if !self.chain.advance() {
            return false;
        }
        if self.deferred || !self.options.defer {
            return true;
        }
        let mut control = self.options.control.lock().unwrap();
        let mut values = Vec::new();
        let mut positions = Vec::new();
        for (parent, position) in self.parents.iter().zip(&self.positions) {
            if control.is_failed(position.group) {
                continue;
            }
            let group = 100 + control.next_group;
            control.next_group += 1;
            control.group_parents.insert(group, position.group);
            values.push(*parent);
            positions.push(Position {
                group,
                deferred: true,
                ..*position
            });
        }
        drop(control);
        if values.is_empty() {
            return true;
        }
        // Deferred scope starts only after its parent's payload was serialized.
        // Its own owning frame adds one depth step, and its resolver's composite
        // outputs become another frame, borrowing the retained item above it.
        self.options
            .control
            .lock()
            .unwrap()
            .deferred_released
            .extend(positions.iter().map(|p| p.group));
        self.chain = Box::new(Frame::from_batch(Box::new(RootBatch::<_, Details, C> {
            values,
            positions,
            args: (),
            ctx: self.ctx,
            options: self.options.clone(),
        })));
        self.deferred = true;
        false
    }
    fn frame_depth(&self) -> usize {
        self.chain.frame_depth()
    }
}
impl<C: Send + Sync, T: Resolver<Name, C> + Resolver<Details, C>> Completes<T, C> for ItemTag {
    fn complete<'a>(
        values: Vec<T>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        T: 'a,
        C: 'a,
    {
        Box::new(Frame::from_batch(Box::new(ItemBatch {
            values,
            positions,
            ctx,
            options,
            non_sync: Cell::new(0),
        })))
    }
}
impl<C: Send + Sync, T: Resolver<DetailName, C>> Completes<T, C> for DetailTag {
    fn complete<'a>(
        values: Vec<T>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        T: 'a,
        C: 'a,
    {
        Box::new(Frame::from_batch(Box::new(RootBatch::<_, DetailName, C> {
            values,
            positions,
            args: (),
            ctx,
            options,
        })))
    }
}

fn main() {
    {
        let owner = String::from("borrowed across owned continuation turns");
        let options = Options {
            control: Arc::default(),
            initial_count: 0,
            capacity: 1,
            incremental: true,
            defer: false,
        };
        let continuation = Box::new(Continuation {
            sources: vec![Some(
                futures::stream::iter([Ok(owner.as_str()), Ok(owner.as_str())]).boxed(),
            )],
            positions: vec![Position {
                group: 0,
                index: 0,
                deferred: false,
            }],
            next_index: vec![0],
            initial_pulled: vec![0],
            cursor: 0,
        });
        let (first, next) = futures::executor::block_on(continuation.run(options.clone()));
        let (second, next) = futures::executor::block_on(next.unwrap().run(options.clone()));
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].0.as_ptr(), owner.as_ptr());
        assert_eq!(second[0].1.index, 1);
        let (last, next) = futures::executor::block_on(next.unwrap().run(options));
        assert!(last.is_empty() && next.is_none());
        println!("PASS owned-continuation: earlier items stay borrowed while later turns run");
    }
    for (
        label,
        count,
        initial_count,
        capacity,
        incremental,
        defer,
        vec_output,
        source_error,
        halt,
        cancel,
        panic_resolver,
    ) in [
        ("stream", 4, 1, 2, true, true, false, false, false, 0, false),
        ("vec", 4, 1, 2, true, true, true, false, false, 0, false),
        (
            "drained", 4, 1, 2, false, false, false, false, false, 0, false,
        ),
        ("zero", 4, 0, 2, true, true, false, false, false, 0, false),
        ("eof", 4, 10, 2, true, true, false, false, false, 0, false),
        (
            "item-error",
            6,
            1,
            2,
            true,
            true,
            false,
            true,
            false,
            0,
            false,
        ),
        ("halt", 6, 1, 2, true, true, false, false, true, 0, false),
        (
            "defer-halt",
            6,
            1,
            2,
            true,
            true,
            false,
            false,
            false,
            0,
            false,
        ),
        (
            "cancel-before",
            4,
            1,
            2,
            true,
            true,
            false,
            false,
            false,
            1,
            false,
        ),
        (
            "cancel-after",
            4,
            1,
            2,
            true,
            true,
            false,
            false,
            false,
            2,
            false,
        ),
        (
            "resolver-panic",
            4,
            1,
            2,
            true,
            true,
            false,
            false,
            false,
            0,
            true,
        ),
        (
            "long", 1000, 1, 1, true, true, false, false, false, 0, false,
        ),
    ] {
        let control: Arc<Mutex<Control>> = Arc::default();
        let options = Options {
            control: control.clone(),
            initial_count,
            capacity,
            incremental,
            defer,
        };
        let ctx = Context(App {
            options: options.clone(),
            count,
            source_error,
            halt,
            gate: AtomicBool::new(cancel != 1),
            panic_resolver,
            defer_halt: label == "defer-halt",
        });
        let parents = ["Ada", "Lin"]
            .into_iter()
            .enumerate()
            .map(|(group, name)| Parent {
                name: name.into(),
                group,
                control: control.clone(),
            })
            .collect();
        let positions = (0..2)
            .map(|group| Position {
                group,
                index: 0,
                deferred: false,
            })
            .collect();
        let owner: Box<dyn Batch + '_> = if vec_output {
            Box::new(RootBatch::<_, ReadyItems, App> {
                values: parents,
                positions,
                args: (),
                ctx: &ctx,
                options,
            })
        } else {
            Box::new(RootBatch::<_, Items, App> {
                values: parents,
                positions,
                args: (),
                ctx: &ctx,
                options,
            })
        };
        let root = Frame::from_batch(owner);
        let mut seen = BTreeSet::new();
        let mut sink_calls = 0;
        let mut data_payloads = 0;
        let mut independent_pace = false;
        let future = borrowed::execute(root, |payload| {
            sink_calls += 1;
            for record in &payload.records {
                let state = control.lock().unwrap();
                let root_group = state
                    .group_parents
                    .get(&record.group)
                    .copied()
                    .unwrap_or(record.group);
                assert!(
                    seen.insert((root_group, record.index, record.deferred)),
                    "duplicate record"
                );
                assert!(!state.is_failed(record.group));
                if record.deferred {
                    assert_ne!(record.group, root_group);
                    assert!(
                        seen.contains(&(root_group, record.index, false)),
                        "defer before parent"
                    );
                }
            }
            if seen.contains(&(0, 1, false)) && !seen.contains(&(1, 1, false)) {
                independent_pace = true;
            }
            if !payload.records.is_empty() {
                if data_payloads == 0 && initial_count > 0 {
                    assert_eq!(
                        payload.records.len(),
                        if incremental {
                            2 * initial_count.min(count)
                        } else {
                            2 * count
                        }
                    );
                }
                data_payloads += 1;
                if cancel == 2 {
                    ctx.0.gate.store(false, Ordering::SeqCst);
                }
            }
            if count < 10 && cancel == 0 && !payload.records.is_empty() {
                println!("{label}: {}", serde_json::to_string(&payload).unwrap());
            }
        });
        let mut future: Pin<Box<dyn Future<Output = ()> + Send + '_>> = Box::pin(future);
        if cancel > 0 {
            let waker = noop_waker();
            let mut cx = TaskContext::from_waker(&waker);
            for _ in 0..32 {
                assert!(future.as_mut().poll(&mut cx).is_pending());
            }
            drop(future);
            assert_eq!(data_payloads, usize::from(cancel == 2));
        } else if panic_resolver {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    futures::executor::block_on(future)
                }))
                .is_err()
            );
        } else {
            futures::executor::block_on(future);
            if source_error || halt {
                assert_eq!(control.lock().unwrap().failure_events.len(), 1);
                for index in 0..count {
                    assert!(seen.contains(&(1, index, false)), "unrelated group lost");
                    assert!(seen.contains(&(1, index, true)), "unrelated defer lost");
                }
                assert!(!seen.contains(&(0, if halt { 1 } else { 2 }, false)));
            } else if label == "defer-halt" {
                let state = control.lock().unwrap();
                assert_eq!(state.failure_events.len(), 1);
                assert!(state.failure_events[0].0 >= 100);
                assert!(
                    !state.is_failed(0),
                    "defer HALT must not kill its parent stream"
                );
                assert_eq!(seen.len(), 4 * count - 1);
                assert!(!seen.contains(&(0, 1, true)));
                assert!(seen.contains(&(0, count - 1, false)));
            } else {
                assert_eq!(seen.len(), 2 * count * if defer { 2 } else { 1 });
            }
        }
        let state = control.lock().unwrap();
        if label == "stream" {
            assert!(independent_pace, "parents must not stream in lockstep");
        }
        assert_eq!(state.live_turns, 0);
        assert_eq!(
            state
                .calls
                .iter()
                .filter(|(name, _)| *name == "items" || *name == "vec-items")
                .count(),
            1
        );
        assert_eq!(state.calls[0].1, 2);
        for (at, event) in state.drops.iter().enumerate() {
            if let Some(rest) = event.strip_prefix("detail:") {
                assert!(
                    at < state
                        .drops
                        .iter()
                        .position(|e| e == &format!("item:{rest}"))
                        .unwrap()
                );
            }
            if let Some(rest) = event
                .strip_prefix("item:")
                .or_else(|| event.strip_prefix("source:"))
            {
                let group = rest.split(':').next().unwrap();
                assert!(
                    at < state
                        .drops
                        .iter()
                        .position(|e| e == &format!("parent:{group}"))
                        .unwrap()
                );
            }
        }
        if source_error {
            assert!(state.drops.contains(&"source:0:3".into()));
        }
        if halt {
            assert!(state.drops.iter().any(|s| s.starts_with("source:0:")));
        }
        assert!(state.max_buffer <= capacity);
        assert!(state.max_depth <= 3); // root adds one; item/defer/detail are nested.
        if label == "long" {
            assert!(state.buffered_while_pending > 0);
            assert!(state.max_turns < 16);
        }
        println!(
            "PASS {label}: barriers={sink_calls} records={} max_turns={} depth_below_root={} pump_while_pending={} failures={:?}",
            seen.len(),
            state.max_turns,
            state.max_depth,
            state.buffered_while_pending,
            state.failure_events
        );
    }
}
