//! THROWAWAY generated-schema + application half. No SDL generator or result arena.
#![allow(non_camel_case_types)]
use greem_completion_probe::*;
use std::{marker::PhantomData, sync::atomic::{AtomicUsize,Ordering}};
struct UserTag; struct PostTag; struct NodeTag;
struct name; impl Field for name { type Type=Text; type Args=(); }
struct posts; impl Field for posts { type Type=List<PostTag>; type Args=(); }
struct author; impl Field for author { type Type=UserTag; type Args=(); }
impl<T: Resolver<name,C> + Resolver<posts,C>,C> Completes<T,C> for UserTag {
    fn complete(value:&T)->Value {
        match <T as Resolver<name,C>>::parent_error(value) { Some(e)=>Value::Error(e.clone()),None=>Value::Object("User") }
    }
}
impl<T: Resolver<author,C>,C> Completes<T,C> for PostTag {
    fn complete(value:&T)->Value {
        match <T as Resolver<author,C>>::parent_error(value) { Some(e)=>Value::Error(e.clone()),None=>Value::Object("Post") }
    }
}
impl<T:Outputs<UserTag,C>,C> Completes<As<UserTag,T>,C> for NodeTag {
    fn complete(value:&As<UserTag,T>)->Value {value.0.complete()}
}
impl<T:Outputs<PostTag,C>,C> Completes<As<PostTag,T>,C> for NodeTag {
    fn complete(value:&As<PostTag,T>)->Value {value.0.complete()}
}
impl<A:Outputs<NodeTag,C>,B:Outputs<NodeTag,C>,C> Completes<Either<A,B>,C> for NodeTag {
    fn complete(value:&Either<A,B>)->Value {match value {Either::A(a)=>a.complete(),Either::B(b)=>b.complete()}}
}
impl<T:Outputs<NodeTag,C>,C> Completes<Result<T,Error>,C> for NodeTag {
    fn complete(value:&Result<T,Error>)->Value {match value {Ok(v)=>v.complete(),Err(e)=>Value::Error(e.clone())}}
}
struct User(String); struct Post(User);
impl<C:Sync> Resolver<name,C> for User {
    type Output<'a>=&'a str where C:'a;
    async fn resolve<'obj,'call>(parents:&'call [&'obj Self],_:&'obj (),_:&'obj Context<C>)
        ->Result<Vec<Self::Output<'obj>>,Error> where 'obj:'call {
        CALLS.fetch_add(1,Ordering::SeqCst);
        Ok(parents.iter().map(|p|p.0.as_str()).collect())
    }
}
impl Resolver<posts,()> for User {
    type Output<'a>=Vec<Post>;
    async fn resolve<'obj,'call>(_:&'call [&'obj Self],_:&'obj (),_:&'obj Context<()>)
        ->Result<Vec<Self::Output<'obj>>,Error> where 'obj:'call {Ok(vec![])}
}
impl Resolver<author,()> for Post {
    type Output<'a>=&'a User;
    async fn resolve<'obj,'call>(p:&'call [&'obj Self],_:&'obj (),_:&'obj Context<()>)
        ->Result<Vec<Self::Output<'obj>>,Error> where 'obj:'call {Ok(p.iter().map(|p|&p.0).collect())}
}
static CALLS:AtomicUsize=AtomicUsize::new(0);
fn main() {
    futures::executor::block_on(async {
        let rows:Vec<Result<User,Error>>=vec![Ok(User("Ada".into())),Err(Error("user unavailable")),Ok(User("Lin".into()))];
        let completed:Vec<_>=rows.iter().map(<UserTag as Completes<_,()>>::complete).collect();
        assert_eq!(completed,vec![Value::Object("User"),Value::Error(Error("user unavailable")),Value::Object("User")]);
        // Completion happens for a __typename-only selection without field calls.
        assert_eq!(CALLS.load(Ordering::SeqCst),0);
        let parents:Vec<_>=rows.iter().zip(&completed).enumerate().filter_map(|(i,(p,v))|matches!(v,Value::Object(_)).then_some((i,p))).collect();
        let ctx=Context(());
        // The temporary slice is gone before these borrowed names are consumed.
        let names={let slice:Vec<_>=parents.iter().map(|(_,p)|*p).collect(); <Result<User,Error> as Resolver<name,()>>::resolve(&slice,&(),&ctx).await.unwrap()};
        assert_eq!(names,vec!["Ada","Lin"]);
        assert_eq!(parents.iter().map(|(i,_)|*i).collect::<Vec<_>>(),vec![0,2]);
        assert_eq!(CALLS.load(Ordering::SeqCst),1);
        println!("object errors completed before fields; successful positions [0, 2] resolved together; borrowed names: {names:?}");
        let nested:Vec<Option<Vec<Result<&User,Error>>>>=vec![Some(vec![Ok(rows[0].as_ref().unwrap()),Err(Error("nested"))]),None];
        let value=<List<Nullable<List<UserTag>>> as Completes<_,()>>::complete(&nested);
        println!("nested object/list/error/null completion: {value:?}");
        assert_eq!(value,Value::List(vec![Value::List(vec![Value::Object("User"),Value::Error(Error("nested"))]),Value::Null]));
        let double:Result<Result<&User,Error>,Error>=Ok(Err(Error("inner")));
        assert_eq!(<UserTag as Completes<_,()>>::complete(&double),Value::Error(Error("inner")));
        let abstract_value:Result<Either<As<UserTag,&User>,As<PostTag,Post>>,Error>=Ok(Either::A(As(rows[0].as_ref().unwrap(),PhantomData)));
        assert_eq!(<NodeTag as Completes<_,()>>::complete(&abstract_value),Value::Object("User"));
        let abstract_error:Result<Either<As<UserTag,&User>,As<PostTag,Post>>,Error>=Err(Error("node"));
        assert_eq!(<NodeTag as Completes<_,()>>::complete(&abstract_error),Value::Error(Error("node")));
        let direct_error=<Result<User,Error> as Resolver<name,()>>::resolve(&[&rows[1]],&(),&ctx).await;
        assert_eq!(direct_error,Err(Error("user unavailable")));
        println!("nested Results, abstract Result, direct erroneous delegation: completed without unreachable bodies");
    });
}
