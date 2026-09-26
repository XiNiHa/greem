#![feature(allocator_api)]
fn assert_send<T: Send>() {}
fn main() {
    assert_send::<bumpalo::Bump>();
    assert_send::<Box<[String], &bumpalo::Bump>>();
}
