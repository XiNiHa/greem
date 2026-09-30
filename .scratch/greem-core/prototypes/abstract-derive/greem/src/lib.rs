pub struct As<Tag, T>(pub T, pub core::marker::PhantomData<Tag>);
pub enum Either<A, B> {
    A(A),
    B(B),
}
pub trait Field {}
pub trait Resolver<F: Field, C = ()> {
    fn resolve(parents: &[&Self]) -> usize;
}
impl<F: Field, C, T: Resolver<F, C>> Resolver<F, C> for &T {
    fn resolve(parents: &[&Self]) -> usize {
        let inner: Vec<&T> = parents.iter().map(|p| **p).collect();
        T::resolve(&inner)
    }
}
#[doc(hidden)]
pub mod __private {
    pub trait Completes<T, C> {
        fn complete(values: Vec<&T>) -> usize;
    }
}
pub trait Outputs<Ty, C> {
    fn complete(values: Vec<&Self>) -> usize;
}
impl<T, Ty: __private::Completes<T, C>, C> Outputs<Ty, C> for T {
    fn complete(values: Vec<&Self>) -> usize {
        Ty::complete(values)
    }
}
pub trait PartitionTyC<Ty, C> {
    fn partition(values: Vec<&Self>) -> usize;
}
pub trait PartitionTy<Ty> {
    fn partition(values: Vec<&Self>) -> usize;
}
pub trait PartitionNoTag {
    fn partition(values: Vec<&Self>) -> usize;
}
