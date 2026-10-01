#[path = "../src/shared.rs"]
mod shared;
use shared::*;

// generated per-tag blanket, next to the As/Either impls
impl<E: greem::PartitionTyC<types::SearchResult, C>, C> greem::__private::Completes<E, C> for types::SearchResult {
    fn complete(values: Vec<&E>) -> usize {
        E::partition(values)
    }
}

fn check<T: greem::Outputs<types::SearchResult, ()>>() {}

fn main() {
    check::<greem::As<types::User, MyUser>>();
}
