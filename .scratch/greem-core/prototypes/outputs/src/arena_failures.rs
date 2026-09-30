// THROWAWAY: compile the exact ownership shapes proposed by the map.
#![allow(dead_code)]
use std::{any::Any, collections::HashMap, future::Future, pin::Pin};
type Fut<'a,T> = Pin<Box<dyn Future<Output=T> + Send + 'a>>;
trait Scope<'req>: Send + Sync {
    fn run(&'req self) -> Fut<'req, Vec<Box<dyn Scope<'req> + 'req>>>;
}
#[cfg(loop_owned)]
async fn execute<'req>(mut generation: Vec<Box<dyn Scope<'req> + 'req>>) {
    while !generation.is_empty() {
        let mut next = Vec::new();
        for scope in &generation { next.extend(scope.run().await); }
        generation = next;
    }
}
#[cfg(owner_borrow)]
fn own_and_borrow<'req>(outputs: Box<[String]>) -> (Box<[String]>, Vec<&'req str>) {
    let refs = outputs.iter().map(String::as_str).collect();
    (outputs, refs)
}
#[cfg(plan_cache)]
fn plan<'req>(cache: &'req mut HashMap<usize, Box<dyn Any + Send + Sync>>) {
    let first: &'req String = cache.entry(0).or_insert_with(|| Box::new(String::from("first"))).downcast_ref().unwrap();
    cache.insert(1, Box::new(String::from("lazy abstract scope")));
    println!("{first}");
}
fn main() {}
