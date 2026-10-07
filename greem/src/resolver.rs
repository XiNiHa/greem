use crate::context::{Context, HintRegistry, Planning};
use crate::error::Error;
use crate::value::FromInput;
use futures::Stream;
use std::future::Future;
use std::marker::PhantomData;

/// A generated field marker: one zero-sized type per schema field.
pub trait Field: 'static + Send + Sync {
    /// The field's arguments, converted from the request.
    type Args: FromInput + Send + Sync + 'static;
    /// The tag encoding the field's GraphQL type (`List<Nullable<types::User>>`).
    type Type;
    const NAME: &'static str;
    const SHAPE: Shape;
}

/// The arguments of field `F`.
pub type Args<F> = <F as Field>::Args;

/// The static nullability shape of a field: one entry per list level and one
/// for the leaf or object at the innermost position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// Nullability per list level, outermost first.
    pub levels: &'static [bool],
    /// Nullability of the innermost position.
    pub inner: bool,
}

impl Shape {
    pub const fn new(levels: &'static [bool], inner: bool) -> Self {
        Self { levels, inner }
    }

    pub(crate) fn nullable_at(&self, level: usize) -> bool {
        if level < self.levels.len() {
            self.levels[level]
        } else {
            self.inner
        }
    }
}

/// The set-based resolution primitive: one impl per schema field on the Rust
/// type that stands for the field's parent object type.
#[diagnostic::on_unimplemented(
    message = "no resolver for GraphQL field `{F}` with context `{C}`",
    label = "implement `greem::Resolver<{F}, {C}>` for this type"
)]
pub trait Resolver<F: Field, C = ()>: Send + Sync {
    type Output<'obj>: Outputs<F::Type, C> + Send
    where
        Self: 'obj,
        C: 'obj;

    fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<F>,
        ctx: &'obj Context<'obj, C>,
    ) -> impl Future<Output = Result<Vec<Self::Output<'obj>>, Error>> + Send + 'call
    where
        'obj: 'call;

    /// Framework hook: an object that failed as a whole. Sealed, since `Seal`
    /// cannot be named outside greem: only the `&T` and `Result<T, Error>`
    /// delegations override it, so every field's impl on one type agrees and
    /// codegen asks any one of them. Users fail an object by returning it as
    /// `Result<T, Error>`.
    #[doc(hidden)]
    fn parent_error(&self, _: sealed::Seal) -> Option<&Error> {
        None
    }

    /// Declares the hint types this field accepts from its descendants.
    fn hints(_registry: &mut HintRegistry<'_>) {}

    /// Runs once per request during lookbehind planning, after every descendant.
    fn plan(_planning: &mut Planning<'_, F, C>) {}
}

impl<T: Resolver<F, C>, F: Field, C: Send + Sync> Resolver<F, C> for &T {
    type Output<'obj>
        = T::Output<'obj>
    where
        Self: 'obj,
        C: 'obj;

    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<F>,
        ctx: &'obj Context<'obj, C>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        let inner: Vec<&'obj T> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner, args, ctx).await
    }

    fn parent_error(&self, seal: sealed::Seal) -> Option<&Error> {
        T::parent_error(self, seal)
    }

    fn hints(registry: &mut HintRegistry<'_>) {
        T::hints(registry)
    }

    fn plan(planning: &mut Planning<'_, F, C>) {
        T::plan(planning)
    }
}

impl<T: Resolver<F, C>, F: Field, C: Send + Sync> Resolver<F, C> for Result<T, Error> {
    type Output<'obj>
        = T::Output<'obj>
    where
        Self: 'obj,
        C: 'obj;

    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<F>,
        ctx: &'obj Context<'obj, C>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        let inner: Result<Vec<&'obj T>, Error> = parents
            .iter()
            .map(|p| p.as_ref().map_err(Clone::clone))
            .collect();
        T::resolve(&inner?, args, ctx).await
    }

    fn parent_error(&self, seal: sealed::Seal) -> Option<&Error> {
        match self {
            Ok(value) => T::parent_error(value, seal),
            Err(error) => Some(error),
        }
    }

    fn hints(registry: &mut HintRegistry<'_>) {
        T::hints(registry)
    }

    fn plan(planning: &mut Planning<'_, F, C>) {
        T::plan(planning)
    }
}

/// The user-facing bound: a Rust output type can be completed as the GraphQL
/// type tagged `Ty`. The single impl bridges to the tag-side [`Completes`].
pub trait Outputs<Ty, C = ()>: Sized + sealed::Sealed<Ty, C> {
    #[doc(hidden)]
    fn __walk<'a>(
        w: &mut crate::plan::Walker<'_, C>,
        node: crate::tree::NodeId,
        leaf: &crate::plan::Leaf,
    ) -> Result<(), crate::tree::Abort>
    where
        Self: 'a,
        C: 'a;

    #[doc(hidden)]
    fn __complete<'a>(
        values: Vec<Self>,
        positions: Vec<crate::exec::complete::Pos>,
        cc: &mut crate::exec::complete::Completion<'a, '_, C>,
    ) where
        Self: 'a,
        C: 'a;

    #[doc(hidden)]
    fn __parent_error(&self) -> Option<&Error>;

    #[doc(hidden)]
    fn __null_leaf(&self) -> bool;

    #[doc(hidden)]
    fn __first_error(
        &self,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error>;

    #[doc(hidden)]
    #[cfg(feature = "reference-executor")]
    fn __reference<'v, 's: 'v>(
        value: Self,
        rc: &crate::exec::reference::RefCompletion<'s, C>,
    ) -> futures::future::BoxFuture<'v, crate::exec::reference::RefValue>
    where
        Self: 'v,
        C: 'v;

    #[doc(hidden)]
    fn __start_fields<'a>(
        cx: &crate::exec::complete::FieldsCx<'a, C>,
        parents: &[&'a Self],
        set: usize,
    ) -> Vec<crate::exec::scope::FieldFuture<'a>>
    where
        Self: 'a,
        C: 'a;

    #[doc(hidden)]
    const __TYPENAME: &'static str;
}

impl<T, Ty: Completes<T, C>, C> Outputs<Ty, C> for T {
    fn __parent_error(&self) -> Option<&Error> {
        Ty::parent_error(self)
    }

    fn __null_leaf(&self) -> bool {
        Ty::null_leaf(self)
    }

    fn __first_error(
        &self,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        Ty::first_error(self, indices, wanted)
    }

    fn __walk<'a>(
        w: &mut crate::plan::Walker<'_, C>,
        node: crate::tree::NodeId,
        leaf: &crate::plan::Leaf,
    ) -> Result<(), crate::tree::Abort>
    where
        Self: 'a,
        C: 'a,
    {
        Ty::walk(w, node, leaf)
    }

    fn __complete<'a>(
        values: Vec<Self>,
        positions: Vec<crate::exec::complete::Pos>,
        cc: &mut crate::exec::complete::Completion<'a, '_, C>,
    ) where
        Self: 'a,
        C: 'a,
    {
        Ty::complete(values, positions, cc)
    }

    #[cfg(feature = "reference-executor")]
    fn __reference<'v, 's: 'v>(
        value: Self,
        rc: &crate::exec::reference::RefCompletion<'s, C>,
    ) -> futures::future::BoxFuture<'v, crate::exec::reference::RefValue>
    where
        Self: 'v,
        C: 'v,
    {
        Ty::reference(value, rc)
    }

    fn __start_fields<'a>(
        cx: &crate::exec::complete::FieldsCx<'a, C>,
        parents: &[&'a Self],
        set: usize,
    ) -> Vec<crate::exec::scope::FieldFuture<'a>>
    where
        Self: 'a,
        C: 'a,
    {
        Ty::start_fields(cx, parents, set)
    }

    const __TYPENAME: &'static str = Ty::TYPENAME;
}
impl<T, Ty: Completes<T, C>, C> sealed::Sealed<Ty, C> for T {}

mod sealed {
    pub trait Sealed<Ty, C> {}

    #[derive(Clone, Copy)]
    pub struct Seal;
}

/// The token generated code passes to [`Resolver::parent_error`].
pub fn seal() -> sealed::Seal {
    sealed::Seal
}

pub use crate::exec::complete::Completes;

/// Wraps a concrete value returned at an abstract-typed position ("`T` as `Tag`").
pub struct As<Tag, T>(pub T, PhantomData<fn() -> Tag>);

impl<Tag, T> As<Tag, T> {
    pub fn new(value: T) -> Self {
        Self(value, PhantomData)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

/// One of two output shapes at an abstract-typed position.
pub enum Either<A, B> {
    A(A),
    B(B),
}

/// A list output produced lazily. Drained in place when the request does not
/// `@stream` the field.
pub struct Streamed<S: Stream, Item = <S as Stream>::Item>(pub(crate) S, PhantomData<Item>);

impl<S: Stream> Streamed<S> {
    pub fn new(source: S) -> Self {
        Self(source, PhantomData)
    }
}

/// Tag for a nullable position wrapping the tag of its inner type.
pub struct Nullable<Ty>(PhantomData<Ty>);

/// Tag for a list position wrapping the tag of its item type.
pub struct List<Ty>(PhantomData<Ty>);

/// The root value for a schema without a mutation type.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMutation;

/// The `Mutation` tag of a schema that declares no mutation type.
pub struct NoMutationType;
