//! THROWAWAY runtime half; generated schema lives in the separate binary crate.
use std::{future::Future, marker::PhantomData};
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub &'static str);
pub trait Field: 'static {
    type Type;
    type Args: Sync;
}
pub struct Context<C>(pub C);
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
    // Agreed: codegen picks one fixed field as its object witness.
    // Normal user implementations inherit None. Wrapper delegation forwards it.
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
        let inner: Vec<&'obj T> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner, args, ctx).await
    }
    fn parent_error(&self) -> Option<&Error> {
        T::parent_error(self)
    }
}
impl<T: Resolver<F, C>, F: Field, C: Send + Sync> Resolver<F, C> for Result<T, Error> {
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
        // Completion filters failed objects before scope construction. A direct
        // caller passing Err still gets an error, never an unreachable panic.
        let inner: Result<Vec<&'obj T>, Error> = parents
            .iter()
            .map(|p| p.as_ref().map_err(Clone::clone))
            .collect();
        T::resolve(&inner?, args, ctx).await
    }
    fn parent_error(&self) -> Option<&Error> {
        match self {
            Ok(value) => T::parent_error(value),
            Err(error) => Some(error),
        }
    }
}
use futures::future::{BoxFuture, join_all};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub enum Slot<'a> {
    Text(&'a str),
    Null,
    Error(&'static str),
    List(usize),
    Object(&'static str),
}
#[derive(Debug, Serialize)]
pub struct Record<'a> {
    pub path: String,
    pub slot: Slot<'a>,
}
#[derive(Serialize)]
pub struct Response<'a> {
    pub records: Vec<Record<'a>>,
}
pub type Scopes<'a, C> = Vec<Box<dyn Scope<'a, C> + 'a>>;
pub type Batches<'a, C> = Vec<Box<dyn Batch<C> + 'a>>;
pub trait Scope<'a, C>: Send + Sync {
    fn run(&self, ctx: &'a Context<C>) -> BoxFuture<'_, Batches<'a, C>>;
}
// Owned output batches only need Send. Shared projected objects need Sync.
pub trait Batch<C>: Send {
    fn prepare<'a>(&'a self, response: &mut Response<'a>, ctx: &'a Context<C>) -> Scopes<'a, C>;
}
pub trait Completes<T, C> {
    fn complete<'a>(
        values: Vec<(&'a T, String)>,
        node: usize,
        response: &mut Response<'a>,
        ctx: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        T: 'a,
        C: 'a;
}
pub trait Outputs<Ty, C> {
    fn complete<'a>(
        values: Vec<(&'a Self, String)>,
        node: usize,
        response: &mut Response<'a>,
        ctx: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        Self: 'a,
        C: 'a;
}
impl<T, Ty: Completes<T, C>, C> Outputs<Ty, C> for T {
    fn complete<'a>(
        values: Vec<(&'a Self, String)>,
        node: usize,
        response: &mut Response<'a>,
        ctx: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        Self: 'a,
        C: 'a,
    {
        Ty::complete(values, node, response, ctx)
    }
}
pub struct Column<T, Ty> {
    pub values: Vec<T>,
    pub paths: Vec<String>,
    pub node: usize,
    pub tag: PhantomData<fn() -> Ty>,
}
impl<T: Outputs<Ty, C> + Send, Ty, C> Batch<C> for Column<T, Ty> {
    fn prepare<'a>(&'a self, response: &mut Response<'a>, ctx: &'a Context<C>) -> Scopes<'a, C> {
        assert_eq!(self.values.len(), self.paths.len());
        T::complete(
            self.values.iter().zip(self.paths.iter().cloned()).collect(),
            self.node,
            response,
            ctx,
        )
    }
}
pub fn execute<'a, C: Send + Sync, F, R>(
    batches: Batches<'a, C>,
    ctx: &'a Context<C>,
    response: Response<'a>,
    finish: F,
) -> BoxFuture<'a, R>
where
    F: for<'r> FnOnce(Response<'r>) -> R + Send + 'a,
    R: Send + 'a,
{
    Box::pin(async move {
        // Reconstruct to shorten covariant borrows to this retained frame.
        let mut response = Response {
            records: response.records.into_iter().collect(),
        };
        let scopes: Vec<_> = batches
            .iter()
            .flat_map(|b| b.prepare(&mut response, ctx))
            .collect();
        if scopes.is_empty() {
            return finish(response);
        }
        let next = join_all(scopes.iter().map(|scope| scope.run(ctx)))
            .await
            .into_iter()
            .flatten()
            .collect();
        execute(next, ctx, response, finish).await
    })
}
pub struct Text;
pub struct List<T>(PhantomData<T>);
pub struct Nullable<T>(PhantomData<T>);
impl<C> Completes<String, C> for Text {
    fn complete<'a>(
        values: Vec<(&'a String, String)>,
        _: usize,
        r: &mut Response<'a>,
        _: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        C: 'a,
    {
        r.records.extend(values.into_iter().map(|(v, path)| Record {
            path,
            slot: Slot::Text(v),
        }));
        vec![]
    }
}
impl<C> Completes<&str, C> for Text {
    fn complete<'a>(
        values: Vec<(&'a &str, String)>,
        _: usize,
        r: &mut Response<'a>,
        _: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        C: 'a,
    {
        r.records.extend(values.into_iter().map(|(v, path)| Record {
            path,
            slot: Slot::Text(v),
        }));
        vec![]
    }
}
impl<T: Outputs<Ty, C>, Ty, C> Completes<Vec<T>, C> for List<Ty> {
    fn complete<'a>(
        values: Vec<(&'a Vec<T>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        T: 'a,
        C: 'a,
    {
        let mut flat = vec![];
        for (v, path) in values {
            r.records.push(Record {
                path: path.clone(),
                slot: Slot::List(v.len()),
            });
            flat.extend(
                v.iter()
                    .enumerate()
                    .map(|(i, v)| (v, format!("{path}[{i}]"))),
            );
        }
        T::complete(flat, node, r, ctx)
    }
}
impl<T: Outputs<Ty, C>, Ty, C> Completes<Option<T>, C> for Nullable<Ty> {
    fn complete<'a>(
        values: Vec<(&'a Option<T>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        T: 'a,
        C: 'a,
    {
        let mut some = vec![];
        for (v, path) in values {
            match v {
                Some(v) => some.push((v, path)),
                None => r.records.push(Record {
                    path,
                    slot: Slot::Null,
                }),
            }
        }
        T::complete(some, node, r, ctx)
    }
}
// Concrete runtime tags: disjoint from generated object blanket impls.
impl<T: Outputs<Text, C>, C> Completes<Result<T, Error>, C> for Text {
    fn complete<'a>(
        values: Vec<(&'a Result<T, Error>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<C>,
    ) -> Scopes<'a, C>
    where
        T: 'a,
        C: 'a,
    {
        let mut ok = vec![];
        for (v, path) in values {
            match v {
                Ok(v) => ok.push((v, path)),
                Err(e) => r.records.push(Record {
                    path,
                    slot: Slot::Error(e.0),
                }),
            }
        }
        T::complete(ok, node, r, ctx)
    }
}
pub struct As<Tag, T>(pub T, pub PhantomData<Tag>);
pub enum Either<A, B> {
    A(A),
    B(B),
}
