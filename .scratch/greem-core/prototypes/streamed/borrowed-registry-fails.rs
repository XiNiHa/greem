// THROWAWAY negative probe: a request-wide registry cannot retain a borrow of
// a chain future's local owner. Compile directly with rustc --edition 2024.
use std::sync::{Arc, Mutex};

async fn chain<'request>(registry: Arc<Mutex<Vec<&'request str>>>) {
    let item = String::from("owned by this chain's frame");
    registry.lock().unwrap().push(&item);
    std::future::pending::<()>().await;
}

fn main() {}
