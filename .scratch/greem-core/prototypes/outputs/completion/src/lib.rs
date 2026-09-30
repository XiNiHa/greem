//! THROWAWAY runtime half; generated schema lives in the separate binary crate.
use std::{future::Future, marker::PhantomData};
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub &'static str);
pub trait Field: 'static { type Type; type Args: Sync; }
pub struct Context<C>(pub C);
pub trait Resolver<F: Field, C>: Send + Sync {
    type Output<'a>: Outputs<F::Type, C> + Send where Self: 'a, C: 'a;
    fn resolve<'obj, 'call>(parents: &'call [&'obj Self], args: &'obj F::Args, ctx: &'obj Context<C>)
        -> impl Future<Output=Result<Vec<Self::Output<'obj>>, Error>> + Send + 'call
        where 'obj: 'call;
    // Agreed: codegen picks one fixed field as its object witness.
    // Normal user implementations inherit None. Wrapper delegation forwards it.
    fn parent_error(&self) -> Option<&Error> { None }
}
impl<T: Resolver<F,C>, F: Field, C: Send + Sync> Resolver<F,C> for &T {
    type Output<'a> = T::Output<'a> where Self:'a, C:'a;
    async fn resolve<'obj,'call>(parents: &'call [&'obj Self], args: &'obj F::Args, ctx: &'obj Context<C>)
        -> Result<Vec<Self::Output<'obj>>,Error> where 'obj:'call {
        let inner: Vec<&'obj T> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner,args,ctx).await
    }
    fn parent_error(&self) -> Option<&Error> { T::parent_error(self) }
}
impl<T: Resolver<F,C>, F: Field, C: Send + Sync> Resolver<F,C> for Result<T,Error> {
    type Output<'a> = T::Output<'a> where Self:'a, C:'a;
    async fn resolve<'obj,'call>(parents: &'call [&'obj Self], args: &'obj F::Args, ctx: &'obj Context<C>)
        -> Result<Vec<Self::Output<'obj>>,Error> where 'obj:'call {
        // Completion filters failed objects before scope construction. A direct
        // caller passing Err still gets an error, never an unreachable panic.
        let inner: Result<Vec<&'obj T>,Error> = parents.iter().map(|p| p.as_ref().map_err(Clone::clone)).collect();
        T::resolve(&inner?,args,ctx).await
    }
    fn parent_error(&self) -> Option<&Error> {
        match self { Ok(value) => T::parent_error(value), Err(error) => Some(error) }
    }
}
#[derive(Debug,PartialEq)]
pub enum Value { Text(String), Null, Error(Error), List(Vec<Value>), Object(&'static str) }
pub trait Completes<T,C> {
    fn complete(value:&T) -> Value;
}
pub trait Outputs<Ty,C> { fn complete(&self) -> Value; }
impl<T,Ty:Completes<T,C>,C> Outputs<Ty,C> for T {
    fn complete(&self) -> Value { Ty::complete(self) }
}
pub struct Text;
pub struct List<T>(PhantomData<T>);
pub struct Nullable<T>(PhantomData<T>);
impl<C> Completes<String,C> for Text { fn complete(value:&String)->Value {Value::Text(value.clone())} }
impl<C> Completes<&str,C> for Text { fn complete(value:&&str)->Value {Value::Text((*value).into())} }
impl<T: Outputs<Text,C>,C> Completes<Result<T,Error>,C> for Text {
    fn complete(value:&Result<T,Error>)->Value {match value {Ok(v)=>v.complete(),Err(e)=>Value::Error(e.clone())}}
}
impl<T: Outputs<Ty,C>,Ty,C> Completes<Vec<T>,C> for List<Ty> {
    fn complete(value:&Vec<T>)->Value {Value::List(value.iter().map(T::complete).collect())}
}
impl<T: Outputs<List<Ty>,C>,Ty,C> Completes<Result<T,Error>,C> for List<Ty> {
    fn complete(value:&Result<T,Error>)->Value {match value {Ok(v)=>v.complete(),Err(e)=>Value::Error(e.clone())}}
}
impl<T: Outputs<Ty,C>,Ty,C> Completes<Option<T>,C> for Nullable<Ty> {
    fn complete(value:&Option<T>)->Value {match value {Some(v)=>v.complete(),None=>Value::Null}}
}
impl<T: Outputs<Nullable<Ty>,C>,Ty,C> Completes<Result<T,Error>,C> for Nullable<Ty> {
    fn complete(value:&Result<T,Error>)->Value {match value {Ok(v)=>v.complete(),Err(e)=>Value::Error(e.clone())}}
}
pub struct As<Tag,T>(pub T,pub PhantomData<Tag>);
pub enum Either<A,B> { A(A),B(B) }
