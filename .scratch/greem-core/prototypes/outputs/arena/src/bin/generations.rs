//! Safe alternative: retain each generation in an async frame until all descendants finish.
//! Runs breadth-first but stores O(depth) nested futures instead of replacing one Vec in a loop.
//! Each child borrows its actual parent's lifetime, not one universal `req` lifetime.
use std::{future::Future, pin::Pin, sync::{Arc, atomic::{AtomicUsize, Ordering}}};
type Fut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
trait Scope: Send + Sync {
    fn run(&self) -> Fut<'_, Vec<Box<dyn Scope + '_>>>;
}
fn execute<'a>(generation: Vec<Box<dyn Scope + 'a>>) -> Fut<'a, ()> {
    Box::pin(async move {
        let results = futures::future::join_all(generation.iter().map(|scope| scope.run())).await;
        let next: Vec<_> = results.into_iter().flatten().collect();
        if !next.is_empty() { execute(next).await; }
    })
}
struct Root<'a> { request: &'a str, owned: String, stall: bool, drops: Arc<AtomicUsize> }
struct Child<'a> { request: &'a str, parent: &'a str, stall: bool, drops: Arc<AtomicUsize> }
impl Scope for Root<'_> {
    fn run(&self) -> Fut<'_, Vec<Box<dyn Scope + '_>>> {
        Box::pin(async move {
            let child: Box<dyn Scope + '_> = Box::new(Child { request: self.request, parent: &self.owned, stall: self.stall, drops: self.drops.clone() });
            vec![child]
        })
    }
}
impl Scope for Child<'_> {
    fn run(&self) -> Fut<'_, Vec<Box<dyn Scope + '_>>> {
        Box::pin(async move { assert_eq!((self.request, self.parent), ("request", "parent")); if self.stall { futures::future::pending::<()>().await; } vec![] })
    }
}
impl Drop for Root<'_> { fn drop(&mut self) { assert_eq!(self.drops.fetch_add(1, Ordering::SeqCst), 1); } }
impl Drop for Child<'_> { fn drop(&mut self) { assert_eq!(self.parent, "parent"); assert_eq!(self.drops.fetch_add(1, Ordering::SeqCst), 0); } }
fn assert_send<T: Send>(_: &T) {}
fn main() {
    let request = String::from("request");
    let drops = Arc::new(AtomicUsize::new(0));
    let run = execute(vec![Box::new(Root { request: &request, owned: "parent".to_owned(), stall: false, drops: drops.clone() })]);
    assert_send(&run);
    futures::executor::block_on(run);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    let cancelled_drops = Arc::new(AtomicUsize::new(0));
    futures::executor::block_on(async {
        let mut pending_run = execute(vec![Box::new(Root { request: &request, owned: "parent".to_owned(), stall: true, drops: cancelled_drops.clone() })]);
        assert_send(&pending_run);
        assert!(futures::poll!(&mut pending_run).is_pending());
        drop(pending_run);
    });
    assert_eq!(cancelled_drops.load(Ordering::SeqCst), 2);
    println!("completion + cancellation: dyn Scope + joined Send futures + borrowed request and parent + child-before-parent Drop");
}
