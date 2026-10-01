#[path = "../src/shared.rs"]
mod shared;
use shared::*;

// generated per-tag blanket, next to the As/Either impls
impl<E: greem::PartitionTyC<types::SearchResult, C> + greem::PartitionTy<types::SearchResult>, C> greem::__private::Completes<E, C> for types::SearchResult {
    fn complete(values: Vec<&E>) -> usize {
        <E as greem::PartitionTyC<types::SearchResult, C>>::partition(values)
    }
}

pub enum Hit<'a> {
    User(&'a MyUser),
    Post(&'a MyPost),
}

// what the derive would emit: a C-less marker (makes the blanket coherent) plus
// the real C-carrying partition.
impl<'a> greem::PartitionTy<types::SearchResult> for Hit<'a> {
    fn partition(values: Vec<&Self>) -> usize {
        <Self as greem::PartitionTyC<types::SearchResult, ()>>::partition(values)
    }
}
impl<'a, C> greem::PartitionTyC<types::SearchResult, C> for Hit<'a>
where
    &'a MyUser: greem::Outputs<types::User, C>,
    &'a MyPost: greem::Outputs<types::Post, C>,
{
    fn partition(values: Vec<&Self>) -> usize {
        let mut users = Vec::new();
        let mut posts = Vec::new();
        for v in values {
            match v {
                Hit::User(u) => users.push(u),
                Hit::Post(p) => posts.push(p),
            }
        }
        <&'a MyUser as greem::Outputs<types::User, C>>::complete(users)
            + <&'a MyPost as greem::Outputs<types::Post, C>>::complete(posts)
    }
}

fn check<T: greem::Outputs<types::SearchResult, ()>>() {}

fn main() {
    check::<greem::As<types::User, MyUser>>();
    check::<greem::Either<greem::As<types::User, MyUser>, greem::As<types::Post, MyPost>>>();
    check::<Hit<'static>>();
}
