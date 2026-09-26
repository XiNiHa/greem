//! A valid Send + Sync borrowed output shape whose destructor dependencies can cycle.
//! Deliberately leak the two allocations: do NOT attempt to demonstrate destruction with UB.
use std::sync::OnceLock;
struct A<'a> { name: String, peer: OnceLock<&'a B<'a>> }
struct B<'a> { name: String, peer: &'a A<'a> }
impl Drop for A<'_> {
    fn drop(&mut self) { if let Some(peer) = self.peer.get() { println!("{}", peer.name); } }
}
impl Drop for B<'_> {
    fn drop(&mut self) { println!("{}", self.peer.name); }
}
fn main() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<A<'_>>();
    assert_send_sync::<B<'_>>();
    let a: &'static A<'static> = Box::leak(Box::new(A { name: "a".to_owned(), peer: OnceLock::new() }));
    let b: &'static B<'static> = Box::leak(Box::new(B { name: "b".to_owned(), peer: a }));
    assert!(a.peer.set(b).is_ok());
    println!("Send + Sync permits cyclic destructor dependencies; leaked intentionally");
}
