pub mod types {
    pub struct User;
    pub struct Post;
    pub struct SearchResult;
}
pub struct UserName;
impl greem::Field for UserName {}
pub struct PostTitle;
impl greem::Field for PostTitle {}

impl<T: greem::Resolver<UserName, C>, C> greem::__private::Completes<T, C> for types::User {
    fn complete(values: Vec<&T>) -> usize {
        values.len()
    }
}
impl<T: greem::Resolver<PostTitle, C>, C> greem::__private::Completes<T, C> for types::Post {
    fn complete(values: Vec<&T>) -> usize {
        values.len()
    }
}

impl<T: greem::Outputs<types::User, C>, C> greem::__private::Completes<greem::As<types::User, T>, C>
    for types::SearchResult
{
    fn complete(values: Vec<&greem::As<types::User, T>>) -> usize {
        T::complete(values.into_iter().map(|v| &v.0).collect())
    }
}
impl<T: greem::Outputs<types::Post, C>, C> greem::__private::Completes<greem::As<types::Post, T>, C>
    for types::SearchResult
{
    fn complete(values: Vec<&greem::As<types::Post, T>>) -> usize {
        T::complete(values.into_iter().map(|v| &v.0).collect())
    }
}
impl<A: greem::Outputs<types::SearchResult, C>, B: greem::Outputs<types::SearchResult, C>, C>
    greem::__private::Completes<greem::Either<A, B>, C> for types::SearchResult
{
    fn complete(values: Vec<&greem::Either<A, B>>) -> usize {
        let mut a = Vec::new();
        let mut b = Vec::new();
        for v in values {
            match v {
                greem::Either::A(x) => a.push(x),
                greem::Either::B(x) => b.push(x),
            }
        }
        A::complete(a) + B::complete(b)
    }
}

pub struct MyUser;
pub struct MyPost;
impl<C> greem::Resolver<UserName, C> for MyUser {
    fn resolve(p: &[&Self]) -> usize {
        p.len()
    }
}
impl greem::Resolver<PostTitle, ()> for MyPost {
    fn resolve(p: &[&Self]) -> usize {
        p.len()
    }
}
