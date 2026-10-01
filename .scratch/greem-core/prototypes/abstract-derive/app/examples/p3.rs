#[path = "../src/shared.rs"]
mod shared;
use shared::*;

// generated per-tag blanket, next to the As/Either impls
impl<E: greem::PartitionTy<types::SearchResult>, C> greem::__private::Completes<E, C> for types::SearchResult {
    fn complete(values: Vec<&E>) -> usize {
        E::partition(values)
    }
}

pub enum Hit<'a> {
    User(&'a MyUser),
    Post(&'a MyPost),
}

// what a derive would have to emit against a C-less entry trait: no C in scope,
// so the inner completions can only be pinned to one context (here `()`),
// yet the blanket above hands out `Completes<Hit, C>` for EVERY C.
impl<'a> greem::PartitionTy<types::SearchResult> for Hit<'a> {
    fn partition(values: Vec<&Self>) -> usize {
        let mut users = Vec::new();
        let mut posts = Vec::new();
        for v in values {
            match v {
                Hit::User(u) => users.push(u),
                Hit::Post(p) => posts.push(p),
            }
        }
        <&'a MyUser as greem::Outputs<types::User, ()>>::complete(users)
            + <&'a MyPost as greem::Outputs<types::Post, ()>>::complete(posts)
    }
}

fn check_string<T: greem::Outputs<types::SearchResult, String>>() {}

fn check<T: greem::Outputs<types::SearchResult, ()>>() {}

fn main() {
    check::<greem::As<types::User, MyUser>>();
    check::<Hit<'static>>();
    // accepted although MyPost has no `Resolver<PostTitle, String>`: the C-less
    // trait cannot propagate the context requirement.
    check_string::<Hit<'static>>();
}
