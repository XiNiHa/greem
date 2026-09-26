// THROWAWAY: explicit, generated per-field adapter alongside blanket delegation.
#![allow(dead_code, non_camel_case_types)]
use probe_runtime::*;
struct name; impl Field for name { type Type = Text; }
struct friends; impl Field for friends { type Type = List<Text>; }
struct User(String);
impl<C: Sync> ObjectResolver<name, C> for User {
    type Output<'a> = &'a str where C: 'a;
    async fn resolve<'a>(&'a self, _: &'a C) -> &'a str { &self.0 }
}
// Macro output, or a manual per-field bridge, instead of a blanket on all T.
impl<C: Sync> Resolver<name, C> for User {
    type Output<'a> = Result<<Self as ObjectResolver<name, C>>::Output<'a>, String> where C: 'a;
    async fn resolve<'a>(parents: &'a [&'a Self], ctx: &'a C) -> Vec<Self::Output<'a>> {
        // Sequential here solely to probe coherence; actual sugar must join.
        let mut outputs = Vec::new();
        for parent in parents { outputs.push(Ok(<Self as ObjectResolver<name,C>>::resolve(parent, ctx).await)); }
        outputs
    }
}
impl Resolver<friends, ()> for User {
    type Output<'a> = Vec<String>;
    async fn resolve<'a>(_: &'a [&'a Self], _: &'a ()) -> Vec<Self::Output<'a>> { vec![] }
}
fn requirements<T: Resolver<name, ()> + Resolver<friends, ()>>() {}
fn main() {
    requirements::<User>();
    requirements::<&User>();
    requirements::<Result<User, String>>();
    println!("explicit per-field sugar + manual set-based field + wrapper delegation: compile");
}
