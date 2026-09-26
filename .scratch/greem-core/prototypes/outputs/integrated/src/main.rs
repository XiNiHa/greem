//! THROWAWAY hand-generated schema and application in a downstream crate.
#![allow(non_camel_case_types)]
use futures::future::BoxFuture;
use greem_integrated_probe::*;
use std::{
    any::Any,
    marker::PhantomData,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
struct QueryTag;
struct UserTag;
struct PostTag;
struct NodeTag;
struct ResourceTag;
struct hits;
impl Field for hits {
    type Type = List<Nullable<List<Nullable<NodeTag>>>>;
    type Args = ();
}
struct name;
impl Field for name {
    type Type = Text;
    type Args = ();
}
struct posts;
impl Field for posts {
    type Type = List<PostTag>;
    type Args = ();
}
struct author;
impl Field for author {
    type Type = UserTag;
    type Args = ();
}
struct Plan {
    args: (),
    name: bool,
    posts: bool,
    author: bool,
}
struct App {
    cancel: bool,
    plans: Vec<OnceLock<Box<dyn Any + Send + Sync>>>,
    trace: Mutex<Vec<String>>,
    drops: AtomicUsize,
}
impl App {
    fn plan(&self, node: usize) -> &Plan {
        self.plans[node]
            .get_or_init(|| {
                Box::new(Plan {
                    args: (),
                    name: node == 1 || node == 3,
                    posts: node == 1,
                    author: node == 2,
                })
            })
            .downcast_ref()
            .unwrap()
    }
}
struct Query<'r>(&'r str);
struct User<'r> {
    name: String,
    request: &'r str,
}
struct Post<'a> {
    owner: &'a str,
    request: &'a str,
    app: &'a App,
}
impl Drop for Post<'_> {
    fn drop(&mut self) {
        assert!(!self.owner.is_empty());
        assert_eq!(self.request, "request");
        self.app.drops.fetch_add(1, Ordering::SeqCst);
    }
}
type Hit<'a> = Either<
    As<ResourceTag, As<UserTag, Result<User<'a>, Error>>>,
    As<UserTag, Result<User<'a>, Error>>,
>;
impl<'r> Resolver<hits, App> for Query<'r> {
    type Output<'a>
        = Vec<Option<Vec<Option<Hit<'a>>>>>
    where
        Self: 'a;
    async fn resolve<'o, 'c>(
        p: &'c [&'o Self],
        _: &'o (),
        ctx: &'o Context<App>,
    ) -> Result<Vec<Self::Output<'o>>, Error>
    where
        'o: 'c,
    {
        ctx.0
            .trace
            .lock()
            .unwrap()
            .push(format!("query.hits:{}", p.len()));
        Ok(p.iter()
            .map(|p| {
                vec![
                    Some(vec![
                        Some(Either::A(As(
                            As(
                                Ok(User {
                                    name: "Ada".into(),
                                    request: p.0,
                                }),
                                PhantomData,
                            ),
                            PhantomData,
                        ))),
                        Some(Either::A(As(
                            As(Err(Error("missing user")), PhantomData),
                            PhantomData,
                        ))),
                        None,
                        Some(Either::A(As(
                            As(
                                Ok(User {
                                    name: "Lin".into(),
                                    request: p.0,
                                }),
                                PhantomData,
                            ),
                            PhantomData,
                        ))),
                        Some(Either::B(As(
                            Ok(User {
                                name: "Bo".into(),
                                request: p.0,
                            }),
                            PhantomData,
                        ))),
                    ]),
                    None,
                ]
            })
            .collect())
    }
}
// Hand-written expansion of the accepted per-object sugar: one concrete impl.
impl<C: Sync> Resolver<name, C> for User<'_> {
    #[cfg(not(wrong))]
    type Output<'a>
        = Result<&'a str, Error>
    where
        Self: 'a,
        C: 'a;
    #[cfg(wrong)]
    type Output<'a>
        = i32
    where
        Self: 'a,
        C: 'a;
    async fn resolve<'o, 'c>(
        p: &'c [&'o Self],
        _: &'o (),
        _: &'o Context<C>,
    ) -> Result<Vec<Self::Output<'o>>, Error>
    where
        'o: 'c,
    {
        #[cfg(not(wrong))]
        {
            Ok(
                futures::future::join_all(p.iter().map(|p| async move { Ok(p.name.as_str()) }))
                    .await,
            )
        }
        #[cfg(wrong)]
        {
            Ok(vec![1; p.len()])
        }
    }
}
impl Resolver<posts, App> for User<'_> {
    type Output<'a>
        = Vec<Post<'a>>
    where
        Self: 'a;
    async fn resolve<'o, 'c>(
        p: &'c [&'o Self],
        _: &'o (),
        ctx: &'o Context<App>,
    ) -> Result<Vec<Self::Output<'o>>, Error>
    where
        'o: 'c,
    {
        ctx.0
            .trace
            .lock()
            .unwrap()
            .push(format!("user.posts:start:{}", p.len()));
        let mut pending = true;
        futures::future::poll_fn(|cx| {
            if pending {
                pending = false;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        })
        .await;
        ctx.0
            .trace
            .lock()
            .unwrap()
            .push(format!("user.posts:end:{}", p.len()));
        Ok(p.iter()
            .map(|p| {
                vec![Post {
                    owner: &p.name,
                    request: p.request,
                    app: &ctx.0,
                }]
            })
            .collect())
    }
}
#[cfg(not(missing))]
impl Resolver<author, App> for Post<'_> {
    type Output<'a>
        = User<'a>
    where
        Self: 'a;
    async fn resolve<'o, 'c>(
        p: &'c [&'o Self],
        _: &'o (),
        ctx: &'o Context<App>,
    ) -> Result<Vec<Self::Output<'o>>, Error>
    where
        'o: 'c,
    {
        ctx.0
            .trace
            .lock()
            .unwrap()
            .push(format!("post.author:{}", p.len()));
        if ctx.0.cancel {
            futures::future::pending::<()>().await;
        }
        Ok(p.iter()
            .map(|p| User {
                name: format!("{} again", p.owner),
                request: p.request,
            })
            .collect())
    }
}
struct QueryScope<'a, T> {
    objects: Vec<&'a T>,
    paths: Vec<String>,
}
impl<'a, T: Resolver<hits, App> + 'a> Scope<'a, App> for QueryScope<'a, T> {
    fn run(&self, ctx: &'a Context<App>) -> BoxFuture<'_, Batches<'a, App>> {
        Box::pin(async move {
            let plan = ctx.0.plan(0);
            let values: Vec<T::Output<'a>> =
                T::resolve(&self.objects, &plan.args, ctx).await.unwrap();
            let next: Batches<'a, App> = vec![Box::new(Column::<
                _,
                List<Nullable<List<Nullable<NodeTag>>>>,
            > {
                values,
                paths: self.paths.iter().map(|p| format!("{p}.hits")).collect(),
                node: 1,
                tag: PhantomData,
            })];
            next
        })
    }
}
struct UserScope<'a, T> {
    objects: Vec<&'a T>,
    paths: Vec<String>,
    node: usize,
}
impl<'a, T: Resolver<name, App> + Resolver<posts, App> + 'a> Scope<'a, App> for UserScope<'a, T> {
    fn run(&self, ctx: &'a Context<App>) -> BoxFuture<'_, Batches<'a, App>> {
        Box::pin(async move {
            let plan = ctx.0.plan(self.node);
            let (names, post_values) = futures::join!(
                async {
                    if plan.name {
                        ctx.0
                            .trace
                            .lock()
                            .unwrap()
                            .push(format!("user.name:{}", self.objects.len()));
                        Some(
                            <T as Resolver<name, App>>::resolve(&self.objects, &plan.args, ctx)
                                .await
                                .unwrap(),
                        )
                    } else {
                        None
                    }
                },
                async {
                    if plan.posts {
                        Some(
                            <T as Resolver<posts, App>>::resolve(&self.objects, &plan.args, ctx)
                                .await
                                .unwrap(),
                        )
                    } else {
                        None
                    }
                }
            );
            let mut next: Batches<'a, App> = vec![];
            if let Some(values) = names {
                next.push(Box::new(Column::<_, Text> {
                    values,
                    paths: self.paths.iter().map(|p| format!("{p}.name")).collect(),
                    node: self.node,
                    tag: PhantomData,
                }));
            }
            if let Some(values) = post_values {
                next.push(Box::new(Column::<_, List<PostTag>> {
                    values,
                    paths: self.paths.iter().map(|p| format!("{p}.posts")).collect(),
                    node: 2,
                    tag: PhantomData,
                }));
            }
            next
        })
    }
}
struct PostScope<'a, T> {
    objects: Vec<&'a T>,
    paths: Vec<String>,
}
impl<'a, T: Resolver<author, App> + 'a> Scope<'a, App> for PostScope<'a, T> {
    fn run(&self, ctx: &'a Context<App>) -> BoxFuture<'_, Batches<'a, App>> {
        Box::pin(async move {
            let plan = ctx.0.plan(2);
            assert!(plan.author);
            let values: Vec<T::Output<'a>> =
                T::resolve(&self.objects, &plan.args, ctx).await.unwrap();
            let next: Batches<'a, App> = vec![Box::new(Column::<_, UserTag> {
                values,
                paths: self.paths.iter().map(|p| format!("{p}.author")).collect(),
                node: 3,
                tag: PhantomData,
            })];
            next
        })
    }
}
impl<T: Resolver<hits, App>> Completes<T, App> for QueryTag {
    fn complete<'a>(
        values: Vec<(&'a T, String)>,
        _: usize,
        r: &mut Response<'a>,
        _: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        T: 'a,
    {
        let mut objects = vec![];
        let mut paths = vec![];
        for (v, path) in values {
            if let Some(e) = T::parent_error(v) {
                r.records.push(Record {
                    path,
                    slot: Slot::Error(e.0),
                });
            } else {
                objects.push(v);
                paths.push(path);
            }
        }
        if objects.is_empty() {
            vec![]
        } else {
            vec![Box::new(QueryScope { objects, paths })]
        }
    }
}
impl<T: Resolver<name, App> + Resolver<posts, App>> Completes<T, App> for UserTag {
    fn complete<'a>(
        values: Vec<(&'a T, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        T: 'a,
    {
        let mut objects = vec![];
        let mut paths = vec![];
        for (v, path) in values {
            if let Some(e) = <T as Resolver<name, App>>::parent_error(v) {
                r.records.push(Record {
                    path,
                    slot: Slot::Error(e.0),
                });
            } else {
                r.records.push(Record {
                    path: path.clone(),
                    slot: Slot::Object("User"),
                });
                objects.push(v);
                paths.push(path);
            }
        }
        let plan = ctx.0.plan(node);
        if objects.is_empty() || (!plan.name && !plan.posts) {
            vec![]
        } else {
            vec![Box::new(UserScope {
                objects,
                paths,
                node,
            })]
        }
    }
}
impl<T: Resolver<author, App>> Completes<T, App> for PostTag {
    fn complete<'a>(
        values: Vec<(&'a T, String)>,
        _: usize,
        r: &mut Response<'a>,
        _: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        T: 'a,
    {
        let mut objects = vec![];
        let mut paths = vec![];
        for (v, path) in values {
            if let Some(e) = T::parent_error(v) {
                r.records.push(Record {
                    path,
                    slot: Slot::Error(e.0),
                });
            } else {
                r.records.push(Record {
                    path: path.clone(),
                    slot: Slot::Object("Post"),
                });
                objects.push(v);
                paths.push(path);
            }
        }
        if objects.is_empty() {
            vec![]
        } else {
            vec![Box::new(PostScope { objects, paths })]
        }
    }
}
impl<T: Outputs<UserTag, App>> Completes<As<UserTag, T>, App> for NodeTag {
    fn complete<'a>(
        values: Vec<(&'a As<UserTag, T>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        T: 'a,
    {
        T::complete(
            values.into_iter().map(|(v, p)| (&v.0, p)).collect(),
            node,
            r,
            ctx,
        )
    }
}
impl<A: Outputs<NodeTag, App>, B: Outputs<NodeTag, App>> Completes<Either<A, B>, App> for NodeTag {
    fn complete<'a>(
        values: Vec<(&'a Either<A, B>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        A: 'a,
        B: 'a,
    {
        let mut a = vec![];
        let mut b = vec![];
        for (v, p) in values {
            match v {
                Either::A(v) => a.push((v, p)),
                Either::B(v) => b.push((v, p)),
            }
        }
        let mut scopes = A::complete(a, node, r, ctx);
        scopes.extend(B::complete(b, node, r, ctx));
        scopes
    }
}
impl<T: Outputs<UserTag, App>> Completes<As<UserTag, T>, App> for ResourceTag {
    fn complete<'a>(
        values: Vec<(&'a As<UserTag, T>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        T: 'a,
    {
        T::complete(
            values.into_iter().map(|(v, p)| (&v.0, p)).collect(),
            node,
            r,
            ctx,
        )
    }
}
impl<T: Outputs<ResourceTag, App>> Completes<As<ResourceTag, T>, App> for NodeTag {
    fn complete<'a>(
        values: Vec<(&'a As<ResourceTag, T>, String)>,
        node: usize,
        r: &mut Response<'a>,
        ctx: &'a Context<App>,
    ) -> Scopes<'a, App>
    where
        T: 'a,
    {
        T::complete(
            values.into_iter().map(|(v, p)| (&v.0, p)).collect(),
            node,
            r,
            ctx,
        )
    }
}
fn main() {
    let request = String::from("request");
    let ctx = Context(App {
        cancel: false,
        plans: (0..5).map(|_| OnceLock::new()).collect(),
        trace: Mutex::new(vec![]),
        drops: AtomicUsize::new(0),
    });
    let root: Batches<'_, App> = vec![Box::new(Column::<_, QueryTag> {
        values: vec![Query(&request)],
        paths: vec!["data".into()],
        node: 0,
        tag: PhantomData,
    })];
    let run = execute(root, &ctx, Response { records: vec![] }, |response| {
        assert_eq!(ctx.0.drops.load(Ordering::SeqCst), 0);
        assert!(
            response
                .records
                .iter()
                .any(|r| matches!(r.slot, Slot::Error("missing user")))
        );
        serde_json::to_string_pretty(&response).unwrap()
    });
    fn require_send<T: Send>(_: &T) {}
    require_send(&run);
    let json = futures::executor::block_on(run);
    assert_eq!(ctx.0.drops.load(Ordering::SeqCst), 3);
    let trace = ctx.0.trace.lock().unwrap();
    assert_eq!(
        &trace[..5],
        &[
            "query.hits:1",
            "user.name:2",
            "user.posts:start:2",
            "user.name:1",
            "user.posts:start:1"
        ]
    );
    let last_parent = trace
        .iter()
        .rposition(|s| s.starts_with("user.posts:end:"))
        .unwrap();
    let first_child = trace
        .iter()
        .position(|s| s.starts_with("post.author:"))
        .unwrap();
    assert!(last_parent < first_child);
    println!(
        "{json}\nBFS trace: {trace:?}\nAll 3 borrowed Post destructors ran after serialization."
    );
    let cancelled = Context(App {
        cancel: true,
        plans: (0..5).map(|_| OnceLock::new()).collect(),
        trace: Mutex::new(vec![]),
        drops: AtomicUsize::new(0),
    });
    let root: Batches<'_, App> = vec![Box::new(Column::<_, QueryTag> {
        values: vec![Query(&request)],
        paths: vec!["data".into()],
        node: 0,
        tag: PhantomData,
    })];
    let mut run = execute(root, &cancelled, Response { records: vec![] }, |_| {
        panic!("cancelled request must not serialize")
    });
    futures::executor::block_on(async {
        futures::future::poll_fn(|cx| {
            use std::task::Poll;
            assert!(run.as_mut().poll(cx).is_pending());
            if cancelled
                .0
                .trace
                .lock()
                .unwrap()
                .iter()
                .any(|s| s.starts_with("post.author:"))
            {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
    });
    drop(run);
    assert_eq!(cancelled.0.drops.load(Ordering::SeqCst), 3);
    println!("Cancellation during borrowed Post resolution: all 3 Posts dropped safely.");
}
