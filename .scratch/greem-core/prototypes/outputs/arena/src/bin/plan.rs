use std::{any::Any, sync::OnceLock};
struct Cache { slots: Vec<OnceLock<Box<dyn Any + Send + Sync>>> }
impl Cache {
    fn plan<T: Any + Send + Sync>(&self, slot: usize, init: impl FnOnce() -> T) -> &T {
        self.slots[slot].get_or_init(|| Box::new(init())).downcast_ref().unwrap()
    }
}
fn main() {
    let cache = Cache { slots: (0..2).map(|_| OnceLock::new()).collect() };
    let first: &String = cache.plan(0, || "first".to_owned());
    let second: &String = cache.plan(1, || "lazy abstract scope".to_owned());
    assert_eq!((first.as_str(), second.as_str()), ("first", "lazy abstract scope"));
    println!("fixed OnceLock slots: existing plan references survive lazy initialization");
}
