//! Subscriptions: the subscribe step that turns the operation's one root
//! field into an event source, and the per-event execution that runs each
//! event as the single object of a fresh root scope.

use crate::error::Error;
use crate::exec::complete::{Completes, Completion, FieldsCx, Pos, new_column, root_batch};
use crate::exec::pull::Ship;
use crate::exec::run::run_loop;
use crate::exec::scope::{Batch, FieldFuture};
use crate::exec::state::Shared;
use crate::plan::{Leaf as LeafPath, PlanTable, Walker};
use crate::resolver::{Args, Field, Outputs, Resolver, Shape};
use crate::tree::{Abort, FieldKind, NodeId};
use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use smallvec::SmallVec;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Mutex;
#[cfg(feature = "reference-executor")]
use {
    crate::exec::reference::{RefCompletion, RefValue},
    futures::future::BoxFuture as RefFuture,
};

/// The subscribe step's outcome: the event source, or the error that kept
/// it from being created, which is the subscription's one response.
pub type SubscribeFuture<'a, C> = BoxFuture<'a, Result<Box<dyn Events<'a, C> + 'a>, Error>>;

/// What one event's execution borrows: a fresh per-event `Shared`, the
/// application value and the plan table.
pub struct EventEnv<'e, C> {
    pub shared: &'e Shared,
    pub app: &'e C,
    pub table: &'e PlanTable,
}

impl<C> Clone for EventEnv<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<C> Copy for EventEnv<'_, C> {}

/// A type-erased event source: pulls the next event and executes it as one
/// response through `outlet`.
pub trait Events<'a, C>: Send {
    /// Executes the next event, or returns `false` once the source ended.
    /// The source is polled only here, so a consumer that stops pulling
    /// stops the subscription.
    fn next_event<'s, 'e>(
        &'s mut self,
        env: EventEnv<'e, C>,
        outlet: &'s mut dyn Ship,
    ) -> BoxFuture<'s, bool>
    where
        'a: 'e,
        'e: 's;
}

/// Resolves subscription root field `F` on `root` once and erases its event
/// source. The field's `refused` error is not applied here: it belongs to
/// every event, as graphql-js raises it.
pub fn subscribe<'a, T, F, C>(
    cx: &FieldsCx<'a, C>,
    index: u32,
    root: &'a T,
    args: &'a Result<Args<F>, Error>,
) -> SubscribeFuture<'a, C>
where
    T: Resolver<F, C> + 'a,
    F: Field,
    F::Type: crate::exec::complete::InnerKind,
    C: Send + Sync + 'a,
{
    let cx = cx.clone();
    Box::pin(async move {
        let args = args.as_ref().map_err(Clone::clone)?;
        let ctx = &cx.contexts[index as usize];
        let mut outputs = T::resolve(&[root], args, ctx).await?;
        if outputs.len() != 1 {
            return Err(Error::framework(
                format!(
                    "resolver for field `{}` returned {} outputs for 1 parent",
                    F::NAME,
                    outputs.len()
                ),
                "CARDINALITY",
            ));
        }
        let output = outputs.pop().expect("one output");
        <T::Output<'a> as Outputs<F::Type, C>>::__events(
            output,
            index,
            F::SHAPE,
            <F::Type as crate::exec::complete::InnerKind>::OBJECTS,
        )
    })
}

/// One subscription's event source and where its events complete. Generic
/// over the stream, not its item: the stream is what outlives the request.
pub(crate) struct EventSourceState<S, Ty, C> {
    source: Pin<Box<S>>,
    field: u32,
    shape: Shape,
    objects: bool,
    tag: PhantomData<fn() -> (Ty, C)>,
}

impl<S, Ty, C> EventSourceState<S, Ty, C> {
    pub(crate) fn new(source: S, field: u32, shape: Shape, objects: bool) -> Self {
        Self {
            source: Box::pin(source),
            field,
            shape,
            objects,
            tag: PhantomData,
        }
    }
}

impl<'a, S, T, Ty, C> Events<'a, C> for EventSourceState<S, Ty, C>
where
    S: Stream<Item = Result<T, Error>> + Send + 'a,
    T: Outputs<Ty, C> + Send,
    Ty: 'static,
    C: Send + Sync + 'a,
{
    fn next_event<'s, 'e>(
        &'s mut self,
        env: EventEnv<'e, C>,
        outlet: &'s mut dyn Ship,
    ) -> BoxFuture<'s, bool>
    where
        'a: 'e,
        'e: 's,
    {
        Box::pin(async move {
            let Some(item) = self.source.next().await else {
                return false;
            };
            let root = EventRoot::<T, Ty> {
                item: Mutex::new(Some(item)),
                field: self.field,
                shape: self.shape,
                objects: self.objects,
                tag: PhantomData,
            };
            let batch = root_batch::<EventRoot<T, Ty>, EventRootTag, C>(
                root, env.shared, env.app, env.table, false,
            );
            let mut scope = batch.start();
            run_loop(&mut scope, env.shared, outlet).await;
            true
        })
    }
}

/// The root object of one event's execution: the event, already resolved,
/// at the subscription's root field.
pub(crate) struct EventRoot<T, Ty> {
    item: Mutex<Option<Result<T, Error>>>,
    field: u32,
    shape: Shape,
    objects: bool,
    tag: PhantomData<fn() -> Ty>,
}

/// The tag an event root completes under: its one field completes the
/// event instead of resolving anything.
pub(crate) struct EventRootTag;

impl<T, Ty, C> Completes<EventRoot<T, Ty>, C> for EventRootTag
where
    T: Outputs<Ty, C> + Send,
    Ty: 'static,
    C: Send + Sync,
{
    fn walk<'a>(_: &mut Walker<'_, C>, _: NodeId, _: &LeafPath) -> Result<(), Abort>
    where
        EventRoot<T, Ty>: 'a,
        C: 'a,
    {
        unreachable!("an event root runs under the subscription root's plan entry")
    }

    fn complete<'a>(_: Vec<EventRoot<T, Ty>>, _: Vec<Pos>, _: &mut Completion<'a, '_, C>)
    where
        EventRoot<T, Ty>: 'a,
        C: 'a,
    {
        unreachable!("an event root is never an output")
    }

    fn start_fields<'a>(
        cx: &FieldsCx<'a, C>,
        parents: &[&'a EventRoot<T, Ty>],
        _set: usize,
    ) -> Vec<FieldFuture<'a>>
    where
        EventRoot<T, Ty>: 'a,
        C: 'a,
    {
        let root = parents[0];
        let cx = cx.clone();
        vec![Box::pin(async move {
            let field = root.field;
            let mut column = new_column(field, root.shape, FieldKind::Normal, 1, root.objects);
            let pos = Pos {
                slot: 0,
                object: 0,
                indices: SmallVec::new(),
            };
            let mut cc = Completion::immediate(&cx, field, &mut column, LeafPath::default(), 0);
            let item = root
                .item
                .lock()
                .unwrap()
                .take()
                .expect("one event completes once");
            match (cx.header.fields[field as usize].refused.clone(), item) {
                (Some(refused), _) => cc.error(&pos, refused),
                (None, Err(error)) => cc.error(&pos, error),
                (None, Ok(value)) => T::__complete(vec![value], vec![pos], &mut cc),
            }
            if let Some(mut driver) = column.stream.take() {
                driver.initial(&mut column).await;
                column.stream = Some(driver);
            }
            column
        })]
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        _: EventRoot<T, Ty>,
        _: &RefCompletion<'s, C>,
    ) -> RefFuture<'v, RefValue>
    where
        EventRoot<T, Ty>: 'v,
        C: 'v,
    {
        unreachable!("the reference executor drains event sources itself")
    }
}
