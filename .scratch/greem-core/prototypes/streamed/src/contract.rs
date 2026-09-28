//! THROWAWAY typed completion + streaming integration. Public resolver signature
//! matches the earlier integrated prototype; completion consumes owned outputs.
use crate::borrowed::{Batch, Chain, Record};
use futures::{
    future::BoxFuture,
    stream::{BoxStream, Stream, StreamExt},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    marker::PhantomData,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Poll},
};

#[derive(Clone, Debug, Serialize)]
pub struct Error(pub &'static str);
pub struct Context<C>(pub C);
pub trait Field: 'static {
    type Type;
    type Args: Sync;
}
pub trait Resolver<F: Field, C>: Send + Sync {
    type Output<'a>: Outputs<F::Type, C> + Send
    where
        Self: 'a,
        C: 'a;
    fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj F::Args,
        ctx: &'obj Context<C>,
    ) -> impl Future<Output = Result<Vec<Self::Output<'obj>>, Error>> + Send + 'call
    where
        'obj: 'call;
    fn parent_error(&self) -> Option<&Error> {
        None
    }
}
impl<T: Resolver<F, C>, F: Field, C: Send + Sync> Resolver<F, C> for &T {
    type Output<'a>
        = T::Output<'a>
    where
        Self: 'a,
        C: 'a;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj F::Args,
        ctx: &'obj Context<C>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        let inner: Vec<_> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner, args, ctx).await
    }
    fn parent_error(&self) -> Option<&Error> {
        T::parent_error(self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub group: usize,
    pub index: usize,
    pub deferred: bool,
}
#[derive(Default)]
pub struct Control {
    pub failed: BTreeMap<usize, &'static str>,
    pub failure_events: Vec<(usize, &'static str)>,
    pub calls: Vec<(&'static str, usize)>,
    pub drops: Vec<String>,
    pub live_turns: usize,
    pub max_turns: usize,
    pub max_depth: usize,
    pub buffered_while_pending: usize,
    pub max_buffer: usize,
    pub deferred_released: BTreeSet<usize>,
    pub group_parents: BTreeMap<usize, usize>,
    pub next_group: usize,
}
impl Control {
    pub fn is_failed(&self, mut group: usize) -> bool {
        loop {
            if self.failed.contains_key(&group) {
                return true;
            }
            match self.group_parents.get(&group) {
                Some(parent) => group = *parent,
                None => return false,
            }
        }
    }
}
#[derive(Clone)]
pub struct Options {
    pub control: Arc<Mutex<Control>>,
    pub initial_count: usize,
    pub capacity: usize,
    pub incremental: bool,
    pub defer: bool,
}

pub trait Completes<T, C> {
    fn complete<'a>(
        values: Vec<T>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        T: 'a,
        C: 'a;
}
pub trait Outputs<Ty, C>: Sized {
    fn complete<'a>(
        values: Vec<Self>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        Self: 'a,
        C: 'a;
}
impl<T, Ty: Completes<T, C>, C> Outputs<Ty, C> for T {
    fn complete<'a>(
        values: Vec<Self>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        Self: 'a,
        C: 'a,
    {
        Ty::complete(values, positions, ctx, options)
    }
}

pub struct Streamed<S: Stream, Item = <S as Stream>::Item>(pub S, PhantomData<Item>);
impl<S: Stream> Streamed<S> {
    pub fn new(source: S) -> Self {
        Self(source, PhantomData)
    }
}
pub struct List<Ty>(PhantomData<Ty>);
pub struct Text;

struct Scalars<T> {
    values: Vec<T>,
    positions: Vec<Position>,
    options: Options,
}
impl<T: AsRef<str> + Send> Chain for Scalars<T> {
    fn poll_generation(&mut self, _: &mut TaskContext<'_>) -> Poll<()> {
        Poll::Ready(())
    }
    fn records<'a>(&'a self, output: &mut Vec<Record<'a>>) {
        let control = self.options.control.lock().unwrap();
        for (value, p) in self.values.iter().zip(&self.positions) {
            if !control.is_failed(p.group) {
                output.push(Record {
                    group: p.group,
                    index: p.index,
                    parent: "typed",
                    value: value.as_ref(),
                    deferred: p.deferred,
                });
            }
        }
    }
    fn advance(&mut self) -> bool {
        true
    }
    fn frame_depth(&self) -> usize {
        0
    }
}
impl<T: AsRef<str> + Send, C> Completes<Result<T, Error>, C> for Text {
    fn complete<'a>(
        values: Vec<Result<T, Error>>,
        positions: Vec<Position>,
        _: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        T: 'a,
        C: 'a,
    {
        assert_eq!(values.len(), positions.len());
        let mut good = Vec::new();
        let mut locations = Vec::new();
        for (value, position) in values.into_iter().zip(positions) {
            match value {
                Ok(value) => {
                    good.push(value);
                    locations.push(position);
                }
                Err(Error(message)) => {
                    let mut control = options.control.lock().unwrap();
                    if control.failed.insert(position.group, message).is_none() {
                        control.failure_events.push((position.group, message));
                    }
                }
            }
        }
        Box::new(Scalars {
            values: good,
            positions: locations,
            options,
        })
    }
}

/// Owned async scope. A completed output becomes an inspectable chain; child
/// object fields begin only after the barrier, never from an early parent poll.
pub struct Resolution<'a> {
    pub pending: Option<BoxFuture<'a, Box<dyn Chain + 'a>>>,
    pub child: Option<Box<dyn Chain + 'a>>,
    pub created: bool,
}
impl Chain for Resolution<'_> {
    fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<()> {
        if let Some(pending) = &mut self.pending {
            match pending.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(child) => {
                    self.child = Some(child);
                    self.pending = None;
                    self.created = true;
                    return Poll::Ready(());
                }
            }
        }
        self.child.as_mut().unwrap().poll_generation(cx)
    }
    fn records<'a>(&'a self, output: &mut Vec<Record<'a>>) {
        if !self.created {
            self.child.as_ref().unwrap().records(output);
        }
    }
    fn advance(&mut self) -> bool {
        if self.created {
            self.created = false;
            false
        } else {
            self.child.as_mut().unwrap().advance()
        }
    }
    fn frame_depth(&self) -> usize {
        self.child.as_ref().map_or(0, |child| child.frame_depth())
    }
}

// A turn's batch has already been moved into its generated completion chain.
// This wrapper counts retained sibling chains without affecting their lifetimes.
struct Counted<'a> {
    chain: Box<dyn Chain + 'a>,
    options: Options,
}
impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.options.control.lock().unwrap().live_turns -= 1;
    }
}
impl Chain for Counted<'_> {
    fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<()> {
        self.chain.poll_generation(cx)
    }
    fn records<'a>(&'a self, out: &mut Vec<Record<'a>>) {
        self.chain.records(out);
    }
    fn advance(&mut self) -> bool {
        self.chain.advance()
    }
    fn frame_depth(&self) -> usize {
        self.chain.frame_depth()
    }
}

pub struct Continuation<'a, T> {
    pub sources: Vec<Option<BoxStream<'a, Result<T, Error>>>>,
    pub positions: Vec<Position>,
    pub next_index: Vec<usize>,
    pub initial_pulled: Vec<usize>,
    pub cursor: usize,
}
impl<T> Continuation<'_, T> {
    // Owned continuation stays in the driver's frame across all polls. `poll`
    // makes the pump callable while sibling chains are pending, with no spawn.
    fn pump(
        &mut self,
        cx: &mut TaskContext<'_>,
        initial: bool,
        buffer: &mut Vec<(T, Position)>,
        options: &Options,
    ) {
        let count = self.sources.len();
        let start = self.cursor;
        for offset in 0..count {
            let n = (start + offset) % count;
            let group = self.positions[n].group;
            if options.control.lock().unwrap().is_failed(group) {
                self.sources[n] = None;
                continue;
            }
            while self.sources[n].is_some()
                && if initial {
                    !options.incremental || self.initial_pulled[n] < options.initial_count
                } else {
                    buffer.len() < options.capacity
                }
            {
                match self.sources[n].as_mut().unwrap().as_mut().poll_next(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        self.sources[n] = None;
                    }
                    Poll::Ready(Some(Err(Error(message)))) => {
                        self.sources[n] = None;
                        let mut control = options.control.lock().unwrap();
                        if control.failed.insert(group, message).is_none() {
                            control.failure_events.push((group, message));
                        }
                        drop(control);
                        buffer.retain(|(_, p)| p.group != group);
                    }
                    Poll::Ready(Some(Ok(value))) => {
                        buffer.push((
                            value,
                            Position {
                                group,
                                index: self.next_index[n],
                                deferred: false,
                            },
                        ));
                        self.next_index[n] += 1;
                        self.initial_pulled[n] += usize::from(initial);
                        self.cursor = (n + 1) % count;
                    }
                }
            }
        }
        if !initial {
            let mut control = options.control.lock().unwrap();
            control.max_buffer = control.max_buffer.max(buffer.len());
            assert!(buffer.len() <= options.capacity);
        }
    }
}
impl<T: Send> Continuation<'_, T> {
    pub async fn run(
        mut self: Box<Self>,
        options: Options,
    ) -> (Vec<(T, Position)>, Option<Box<Self>>) {
        let mut completed = Vec::new();
        futures::future::poll_fn(|cx| {
            self.pump(cx, false, &mut completed, &options);
            if !completed.is_empty() || self.sources.iter().all(Option::is_none) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        let next = if self.sources.iter().any(Option::is_some) {
            Some(self)
        } else {
            None
        };
        (completed, next)
    }
}

struct StreamDriver<'a, T, Ty, C> {
    continuation: Box<Continuation<'a, T>>,
    buffer: Vec<(T, Position)>,
    turns: Vec<(BTreeSet<usize>, Box<dyn Chain + 'a>)>,
    initial: bool,
    admitted: bool,
    ctx: &'a Context<C>,
    options: Options,
    tag: PhantomData<fn() -> Ty>,
}
impl<'obj, T: Outputs<Ty, C> + Send + 'obj, Ty: 'static, C: Send + Sync> Chain
    for StreamDriver<'obj, T, Ty, C>
{
    fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<()> {
        self.continuation
            .pump(cx, self.initial, &mut self.buffer, &self.options);
        let initial_ready = !self.initial
            || self.continuation.sources.iter().enumerate().all(|(n, s)| {
                s.is_none()
                    || self.options.incremental
                        && self.continuation.initial_pulled[n] == self.options.initial_count
            });
        if initial_ready && !self.admitted && !self.buffer.is_empty() {
            let (values, positions): (Vec<_>, Vec<_>) =
                std::mem::take(&mut self.buffer).into_iter().unzip();
            let groups = positions.iter().map(|p| p.group).collect();
            // One set-based completion over every parent that produced items in
            // the turn, including a mixed-group initial set.
            let chain = T::complete(values, positions, self.ctx, self.options.clone());
            {
                let mut control = self.options.control.lock().unwrap();
                control.live_turns += 1;
                control.max_turns = control.max_turns.max(control.live_turns);
            }
            self.turns.push((
                groups,
                Box::new(Counted {
                    chain,
                    options: self.options.clone(),
                }),
            ));
            self.admitted = true;
        }
        let mut ready = initial_ready;
        for (_, turn) in &mut self.turns {
            if turn.poll_generation(cx).is_pending() {
                ready = false;
            }
        }
        {
            let mut control = self.options.control.lock().unwrap();
            control.max_depth = control.max_depth.max(self.frame_depth());
            if !ready && !self.initial && !self.buffer.is_empty() {
                control.buffered_while_pending += 1;
            }
        }
        if ready
            && (self.initial
                || !self.turns.is_empty()
                || self.continuation.sources.iter().all(Option::is_none))
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    fn records<'a>(&'a self, output: &mut Vec<Record<'a>>) {
        for (_, turn) in &self.turns {
            turn.records(output);
        }
        let control = self.options.control.lock().unwrap();
        output.retain(|r| !control.is_failed(r.group));
    }
    fn advance(&mut self) -> bool {
        let failed: BTreeSet<_> = self
            .options
            .control
            .lock()
            .unwrap()
            .failed
            .keys()
            .copied()
            .collect();
        let initial_shipped = if self.initial {
            let mut view = Vec::new();
            for (_, turn) in &self.turns {
                turn.records(&mut view);
            }
            !view.is_empty()
                || self.options.initial_count == 0
                || self.turns.is_empty()
                || self
                    .turns
                    .iter()
                    .all(|(groups, _)| groups.iter().all(|g| failed.contains(g)))
        } else {
            false
        };
        self.turns.retain_mut(|(groups, turn)| {
            !groups.iter().all(|g| failed.contains(g)) && !turn.advance()
        });
        self.buffer.retain(|(_, p)| !failed.contains(&p.group));
        for (n, source) in self.continuation.sources.iter_mut().enumerate() {
            if failed.contains(&self.continuation.positions[n].group) {
                *source = None;
            }
        }
        if initial_shipped {
            self.initial = false;
        }
        if !self.initial {
            self.admitted = false;
        }
        self.turns.is_empty()
            && self.buffer.is_empty()
            && self.continuation.sources.iter().all(Option::is_none)
    }
    fn frame_depth(&self) -> usize {
        self.turns
            .iter()
            .map(|(_, t)| t.frame_depth())
            .max()
            .unwrap_or(0)
    }
}

impl<S, T, Ty: 'static, C> Completes<Streamed<S, Result<T, Error>>, C> for List<Ty>
where
    S: Stream<Item = Result<T, Error>> + Send,
    T: Outputs<Ty, C> + Send,
    C: Send + Sync,
{
    fn complete<'a>(
        values: Vec<Streamed<S, Result<T, Error>>>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        Streamed<S, Result<T, Error>>: 'a,
        C: 'a,
    {
        assert!(options.capacity > 0);
        assert_eq!(values.len(), positions.len());
        let count = values.len();
        Box::new(StreamDriver::<T, Ty, C> {
            continuation: Box::new(Continuation {
                sources: values.into_iter().map(|s| Some(s.0.boxed())).collect(),
                positions,
                next_index: vec![0; count],
                initial_pulled: vec![0; count],
                cursor: 0,
            }),
            buffer: Vec::new(),
            turns: Vec::new(),
            initial: true,
            admitted: false,
            ctx,
            options,
            tag: PhantomData,
        })
    }
}
impl<T: Outputs<Ty, C> + Send, Ty: 'static, C: Send + Sync> Completes<Vec<T>, C> for List<Ty> {
    fn complete<'a>(
        values: Vec<Vec<T>>,
        positions: Vec<Position>,
        ctx: &'a Context<C>,
        options: Options,
    ) -> Box<dyn Chain + 'a>
    where
        T: 'a,
        C: 'a,
    {
        let values = values
            .into_iter()
            .map(|items| Streamed::new(futures::stream::iter(items.into_iter().map(Ok))))
            .collect();
        <List<Ty> as Completes<_, C>>::complete(values, positions, ctx, options)
    }
}

/// Generated columns use this concrete owning batch + the single Outputs bridge.
/// Kept here as a utility for downstream generated-code equivalents.
pub struct RootBatch<'a, T, F: Field, C> {
    pub values: Vec<T>,
    pub positions: Vec<Position>,
    pub args: F::Args,
    pub ctx: &'a Context<C>,
    pub options: Options,
}
impl<T, F, C> Batch for RootBatch<'_, T, F, C>
where
    T: Resolver<F, C>,
    F: Field,
    F::Args: Send,
    C: Send + Sync,
{
    fn start(&self) -> Box<dyn Chain + '_> {
        let parents: Vec<_> = self.values.iter().collect();
        let ctx = self.ctx;
        let args = &self.args;
        let options = self.options.clone();
        let positions = self.positions.clone();
        Box::new(Resolution {
            pending: Some(Box::pin(async move {
                let (parents, positions): (Vec<_>, Vec<_>) = {
                    let control = options.control.lock().unwrap();
                    parents
                        .into_iter()
                        .zip(positions)
                        .filter(|(_, p)| !control.is_failed(p.group))
                        .unzip()
                };
                if parents.is_empty() {
                    return Box::new(Scalars::<&str> {
                        values: vec![],
                        positions,
                        options,
                    }) as Box<dyn Chain>;
                }
                let values = T::resolve(&parents, args, ctx).await.unwrap();
                T::Output::complete(values, positions, ctx, options)
            })),
            child: None,
            created: false,
        })
    }
}
