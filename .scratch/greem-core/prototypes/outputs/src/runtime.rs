// THROWAWAY: type/coherence probe, not a production runtime.
#![allow(dead_code)]
use std::future::Future;
pub trait Field: 'static { type Type; }
pub trait Completes<T, C> {}
pub trait Outputs<Ty, C> {}
impl<T, Ty: Completes<T, C>, C> Outputs<Ty, C> for T {}
#[diagnostic::on_unimplemented(message = "no resolver for GraphQL field `{F}` with context `{C}`")]
pub trait Resolver<F: Field, C>: Send + Sync {
    type Output<'a>: Outputs<F::Type, C> + Send where Self: 'a, C: 'a;
    fn resolve<'a>(parents: &'a [&'a Self], ctx: &'a C)
        -> impl Future<Output = Vec<Self::Output<'a>>> + Send + 'a;
}
pub trait ObjectResolver<F: Field, C>: Send + Sync {
    type Output<'a>: Outputs<F::Type, C> + Send where Self: 'a, C: 'a;
    fn resolve<'a>(&'a self, ctx: &'a C)
        -> impl Future<Output = Self::Output<'a>> + Send + 'a;
}
#[cfg(sugar)]
impl<T: ObjectResolver<F, C>, F: Field, C: Sync> Resolver<F, C> for T {
    type Output<'a> = T::Output<'a> where Self: 'a, C: 'a;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a C) -> Vec<Self::Output<'a>> { unimplemented!() }
}
#[cfg(delegation)]
impl<T: Resolver<F, C>, F: Field, C: Sync> Resolver<F, C> for &T {
    type Output<'a> = T::Output<'a> where Self: 'a, C: 'a;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a C) -> Vec<Self::Output<'a>> { unimplemented!() }
}
#[cfg(delegation)]
impl<T: Resolver<F, C>, F: Field, C: Sync> Resolver<F, C> for Result<T, String> {
    type Output<'a> = T::Output<'a> where Self: 'a, C: 'a;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a C) -> Vec<Self::Output<'a>> { unimplemented!() }
}
pub struct List<T>(std::marker::PhantomData<T>);
pub struct Nullable<T>(std::marker::PhantomData<T>);
pub struct Text;
impl<T, Ty, C> Completes<Vec<T>, C> for List<Ty> where T: Outputs<Ty, C> {}
impl<T, Ty, C> Completes<Option<T>, C> for Nullable<Ty> where T: Outputs<Ty, C> {}
impl<C> Completes<String, C> for Text {}
impl<C> Completes<&str, C> for Text {}
pub struct As<Tag, T>(pub T, pub std::marker::PhantomData<Tag>);
pub enum Either<A, B> { A(A), B(B) }
// These are disjoint because Text and List are concrete runtime tags.
impl<T, C> Completes<Result<T, String>, C> for Text where T: Outputs<Text, C> {}
impl<T, Ty, C> Completes<Result<T, String>, C> for List<Ty> where T: Outputs<List<Ty>, C> {}
impl<T, Ty, C> Completes<Result<T, String>, C> for Nullable<Ty> where T: Outputs<Nullable<Ty>, C> {}
