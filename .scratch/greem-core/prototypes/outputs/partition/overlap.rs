// Compile-fail: treating equal generic arm types specially requires overlapping impls.
struct Either<A, B>(A, B);
trait Partition {}
impl<A, B> Partition for Either<A, B> {}
impl<T> Partition for Either<T, T> {}
fn main() {}
