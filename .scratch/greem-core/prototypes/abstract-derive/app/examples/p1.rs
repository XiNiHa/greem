#[path = "../src/shared.rs"]
mod shared;
use shared::*;

pub enum Hit<'a, X> {
    User(&'a MyUser),
    Post(&'a MyPost),
    Extra(X), // #[greem(as = "User")]
}

// what `#[derive(Abstract)]` would emit
impl<'a, X, C> greem::__private::Completes<Hit<'a, X>, C> for types::SearchResult
where
    &'a MyUser: greem::Outputs<types::User, C>,
    &'a MyPost: greem::Outputs<types::Post, C>,
    X: greem::Outputs<types::User, C>,
{
    fn complete(values: Vec<&Hit<'a, X>>) -> usize {
        let mut users = Vec::new();
        let mut posts = Vec::new();
        let mut extra = Vec::new();
        for v in values {
            match v {
                Hit::User(u) => users.push(u),
                Hit::Post(p) => posts.push(p),
                Hit::Extra(x) => extra.push(x),
            }
        }
        <&'a MyUser as greem::Outputs<types::User, C>>::complete(users)
            + <&'a MyPost as greem::Outputs<types::Post, C>>::complete(posts)
            + <X as greem::Outputs<types::User, C>>::complete(extra)
    }
}

fn check<T: greem::Outputs<types::SearchResult, ()>>() {}

fn main() {
    check::<Hit<'static, MyUser>>();
    check::<greem::As<types::User, MyUser>>();
    check::<greem::Either<greem::As<types::User, MyUser>, greem::As<types::Post, MyPost>>>();
}
