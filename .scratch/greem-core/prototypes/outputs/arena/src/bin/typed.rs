use std::{future::Future, pin::Pin};
type Fut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
trait Scope<'r>: Send + Sync {
    fn run(&'r self) -> Fut<'r, Vec<Box<dyn Scope<'r> + 'r>>>;
}
async fn execute<'r>(arena: &'r typed_arena::Arena<Box<dyn Scope<'r> + 'r>>, initial: Box<dyn Scope<'r> + 'r>) {
    let mut generation: Vec<&'r (dyn Scope<'r> + 'r)> = vec![&**arena.alloc(initial)];
    while !generation.is_empty() {
        let results = futures::future::join_all(generation.iter().map(|scope| scope.run())).await;
        generation = results.into_iter().flatten().map(|scope| &**arena.alloc(scope)).collect();
    }
}
struct Root;
impl<'r> Scope<'r> for Root {
    fn run(&'r self) -> Fut<'r, Vec<Box<dyn Scope<'r> + 'r>>> { Box::pin(async { vec![] }) }
}
fn main() {
    let arena = typed_arena::Arena::new();
    futures::executor::block_on(execute(&arena, Box::new(Root)));
}
