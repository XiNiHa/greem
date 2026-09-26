// THROWAWAY: separate downstream crate, hand-written stand-in for codegen.
#![allow(dead_code, non_camel_case_types)]
use probe_runtime::*;
struct UserTag; struct PostTag; struct NodeTag; struct ResourceTag;
struct name; impl Field for name { type Type = Text; }
struct posts; impl Field for posts { type Type = List<Nullable<PostTag>>; }
struct author; impl Field for author { type Type = UserTag; }
impl<T: Resolver<name, C> + Resolver<posts, C>, C> Completes<T, C> for UserTag {}
impl<T: Resolver<author, C>, C> Completes<T, C> for PostTag {}
impl<T: Outputs<UserTag, C>, C> Completes<As<UserTag, T>, C> for NodeTag {}
impl<T: Outputs<PostTag, C>, C> Completes<As<PostTag, T>, C> for NodeTag {}
impl<T: Outputs<UserTag, C>, C> Completes<As<UserTag, T>, C> for ResourceTag {}
impl<T: Outputs<ResourceTag, C>, C> Completes<As<ResourceTag, T>, C> for NodeTag {}
impl<A: Outputs<NodeTag, C>, B: Outputs<NodeTag, C>, C> Completes<Either<A, B>, C> for NodeTag {}
struct User; struct Post;
#[cfg(not(missing))]
impl<C: Sync> Resolver<name, C> for User {
    #[cfg(not(wrong))] type Output<'a> = &'a str where C: 'a;
    #[cfg(wrong)] type Output<'a> = i32 where C: 'a;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a C) -> Vec<Self::Output<'a>> { unimplemented!() }
}
impl Resolver<posts, ()> for User {
    type Output<'a> = Vec<Option<Post>>;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a ()) -> Vec<Self::Output<'a>> { vec![] }
}
impl Resolver<author, ()> for Post {
    type Output<'a> = User;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a ()) -> Vec<Self::Output<'a>> { vec![] }
}
struct Generic<T>(T);
impl<T: Outputs<Text, ()> + Clone + Send + Sync> Resolver<name, ()> for Generic<T> {
    type Output<'a> = T where Self: 'a;
    async fn resolve<'a>(p: &'a [&'a Self], _: &'a ()) -> Vec<Self::Output<'a>> { p.iter().map(|p| p.0.clone()).collect() }
}
impl<T: Send + Sync> Resolver<posts, ()> for Generic<T> {
    type Output<'a> = Vec<Option<Post>> where Self: 'a;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a ()) -> Vec<Self::Output<'a>> { vec![] }
}
enum UserEnum { User(User), Post(Post) }
// This impl would be emitted by a derive in the application crate.
impl Completes<UserEnum, ()> for NodeTag {}
fn boundary<T: Outputs<UserTag, ()>>() {}
fn nested<T: Outputs<List<Nullable<List<Nullable<NodeTag>>>>, ()>>() {}
fn main() {
    boundary::<User>();
    boundary::<Generic<String>>();
    nested::<Vec<Option<Vec<Option<Either<As<ResourceTag, As<UserTag, User>>, As<PostTag, Post>>>>>>>();
    fn derived<T: Outputs<NodeTag, ()>>() {} derived::<UserEnum>();
    println!("mutually recursive objects, nested nullable lists, generic output, mixed contexts, abstract wrappers/sub-interface, derive placement: compile");
}
