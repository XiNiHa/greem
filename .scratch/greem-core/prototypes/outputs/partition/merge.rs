// Compile-fail: both arms having the same tag does not establish A = B.
fn merge<'a, A, B>(left: Vec<&'a A>, right: Vec<&'a B>) -> Vec<&'a A> {
    let mut bucket = left;
    bucket.extend(right);
    bucket
}
fn main() {}
