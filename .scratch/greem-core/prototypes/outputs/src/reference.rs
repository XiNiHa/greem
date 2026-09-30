// THROWAWAY: isolate the parent-slice lifetime in reference delegation.
#![allow(dead_code)]
use std::future::Future;
trait R: Send + Sync {
    type Output<'a>: Send where Self: 'a;
    #[cfg(not(split))]
    fn resolve<'a>(parents: &'a [&'a Self]) -> impl Future<Output=Vec<Self::Output<'a>>> + Send + 'a;
    #[cfg(split)]
    fn resolve<'req, 'call>(parents: &'call [&'req Self]) -> impl Future<Output=Vec<Self::Output<'req>>> + Send + 'call where 'req: 'call;
}
impl<T: R> R for &T {
    type Output<'a> = T::Output<'a> where Self: 'a;
    #[cfg(not(split))]
    async fn resolve<'a>(parents: &'a [&'a Self]) -> Vec<Self::Output<'a>> {
        let inner: Vec<&'a T> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner).await
    }
    #[cfg(split)]
    async fn resolve<'req, 'call>(parents: &'call [&'req Self]) -> Vec<Self::Output<'req>> where 'req: 'call {
        let inner: Vec<&'req T> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner).await
    }
}
fn main() {}
