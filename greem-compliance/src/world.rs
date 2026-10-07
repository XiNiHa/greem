//! A seeded in-memory dataset plus failure map; every resolver is a pure
//! function of it, so the same world drives both executors.

use crate::schema::{self, types};
use greem::{
    Args, As, Context, DeliveryGroup, Either, Error, HintRegistry, Planning, Resolver, Streamed,
};
use std::collections::BTreeSet;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Failure {
    pub type_name: &'static str,
    pub field: &'static str,
    pub object: u32,
}

#[derive(Debug, Default)]
pub struct World {
    pub users: u32,
    pub posts_per_user: u32,
    pub failures: BTreeSet<Failure>,
    pub cardinality_failure: bool,
    /// The interleaving: yield counts consumed per resolver call, cyclically.
    pub yields: Vec<u32>,
    pub calls: std::sync::Arc<Mutex<Vec<(&'static str, usize)>>>,
    /// A field whose resolver stays pending until `open_gate` is called.
    pub gate_field: Option<&'static str>,
    pub gate_open: std::sync::atomic::AtomicBool,
    /// A field whose resolver panics.
    pub panic_field: Option<&'static str>,
    /// Records drops of tracked objects (depth per object) when set.
    pub track_drops: bool,
    /// `User.score` returns NaN.
    pub nan_score: bool,
    /// `User.drafts` of this user is a stream that never yields.
    pub stalled_drafts: Option<u32>,
    /// `Query.numbers` panics if pulled past its failing item.
    pub panic_after_stream_error: bool,
    /// `User.drafts` of this user is a stream that panics when polled.
    pub panic_drafts: Option<u32>,
}

thread_local! {
    pub static DROPS: std::cell::RefCell<Vec<(&'static str, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn take_drops() -> Vec<(&'static str, u32)> {
    DROPS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

impl World {
    pub fn seeded(users: u32, posts_per_user: u32) -> Self {
        World {
            users,
            posts_per_user,
            ..Default::default()
        }
    }

    pub fn fails(&self, type_name: &'static str, field: &'static str, object: u32) -> bool {
        self.failures.contains(&Failure {
            type_name,
            field,
            object,
        })
    }

    fn call(&self, name: &'static str, parents: usize) {
        self.calls.lock().unwrap().push((name, parents));
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    pub fn open_gate(&self) {
        self.gate_open
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn pause(&self, name: &'static str) {
        if self.panic_field == Some(name) {
            panic!("intentional panic in {name}");
        }
        let yields = if self.yields.is_empty() {
            0
        } else {
            let n = self.calls.lock().unwrap().len();
            self.yields[n % self.yields.len()]
        };
        for _ in 0..yields {
            futures::pending_once().await;
        }
        if self.gate_field == Some(name) {
            std::future::poll_fn(|_| {
                if self.gate_open.load(std::sync::atomic::Ordering::SeqCst) {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            })
            .await;
        }
    }
}

mod futures {
    use std::task::Poll;
    pub async fn pending_once() {
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }
}

pub struct QueryRoot;
pub struct MutationRoot;

pub struct User {
    pub id: u32,
    pub depth: u32,
    pub tracked: bool,
    /// Set by `Query.users` when `User.tag` reported a deferred delivery group.
    pub hinted: bool,
}

#[allow(non_snake_case)]
pub fn User(id: u32) -> User {
    User {
        id,
        depth: 0,
        tracked: false,
        hinted: false,
    }
}

/// The hint `User.tag` writes: whether it was selected under a deferred
/// fragment. `Query.users` accepts it and lets it shape the objects it
/// returns, which is exactly what ticket 11 allows and what breaks the
/// incremental fold property.
#[derive(Default)]
pub struct GroupHint {
    pub deferred: bool,
}

impl Drop for User {
    fn drop(&mut self) {
        if self.tracked {
            DROPS.with(|d| d.borrow_mut().push(("User", self.depth)));
        }
    }
}

pub struct Post {
    pub owner: u32,
    pub index: u32,
    pub tracked: bool,
}

impl Drop for Post {
    fn drop(&mut self) {
        if self.tracked {
            DROPS.with(|d| d.borrow_mut().push(("Post", self.index)));
        }
    }
}

impl User {
    pub fn name(&self) -> String {
        format!("user{}", self.id)
    }
}

pub fn uuid_for(n: u32) -> uuid::Uuid {
    uuid::Uuid::from_u128(0x1234_0000_0000_0000_0000_0000_0000_0000u128 + n as u128)
}

macro_rules! resolver {
    ($ty:ty, $marker:path, $name:literal, $out:ty, |$parents:ident, $args:ident, $ctx:ident| $body:expr) => {
        impl Resolver<$marker, World> for $ty {
            type Output<'obj>
                = $out
            where
                Self: 'obj;
            async fn resolve<'obj, 'call>(
                $parents: &'call [&'obj Self],
                $args: &'obj Args<$marker>,
                $ctx: &'obj Context<'obj, World>,
            ) -> Result<Vec<Self::Output<'obj>>, Error>
            where
                'obj: 'call,
            {
                $ctx.app().call($name, $parents.len());
                $ctx.app().pause($name).await;
                let out: Vec<Self::Output<'obj>> = $body;
                if $ctx.app().cardinality_failure && $name == "User.posts" {
                    return Ok(out.into_iter().take(1).collect());
                }
                Ok(out)
            }
        }
    };
}

fn fail_or<T>(
    world: &World,
    ty: &'static str,
    field: &'static str,
    object: u32,
    value: T,
) -> Result<T, Error> {
    if world.fails(ty, field, object) {
        Err(Error::new(format!("{ty}.{field} failed for {object}")))
    } else {
        Ok(value)
    }
}

impl Resolver<schema::Query::users, World> for QueryRoot {
    type Output<'obj>
        = Vec<User>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Query::users>,
        ctx: &'obj Context<'obj, World>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().call("Query.users", parents.len());
        ctx.app().pause("Query.users").await;
        let n = ctx.app().users.min(args.first.unwrap_or(10).max(0) as u32);
        let tracked = ctx.app().track_drops;
        let hinted = ctx.hint::<GroupHint>().deferred;
        Ok(parents
            .iter()
            .map(|_| {
                (0..n)
                    .map(|id| User {
                        id,
                        depth: 0,
                        tracked,
                        hinted,
                    })
                    .collect()
            })
            .collect())
    }
    fn hints(registry: &mut HintRegistry<'_>) {
        registry.accept::<GroupHint>();
    }
}

impl Resolver<schema::User::tag, World> for User {
    type Output<'obj>
        = &'obj str
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, World>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().call("User.tag", parents.len());
        Ok(parents
            .iter()
            .map(|u| if u.hinted { "deferred" } else { "initial" })
            .collect())
    }
    fn plan(planning: &mut Planning<'_, schema::User::tag, World>) {
        let deferred = planning.delivery_group() == DeliveryGroup::Deferred;
        planning.hint::<GroupHint>(|h| h.deferred |= deferred);
    }
}
resolver!(
    QueryRoot,
    schema::Query::user,
    "Query.user",
    Option<User>,
    |parents, args, ctx| {
        parents
            .iter()
            .map(|_| {
                args.id
                    .parse::<u32>()
                    .ok()
                    .filter(|&id| id < ctx.app().users)
                    .map(User)
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::node,
    "Query.node",
    Option<Either<As<types::User, User>, As<types::Post, Post>>>,
    |parents, args, ctx| {
        parents
            .iter()
            .map(|_| node_for(ctx.app(), &args.id))
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::nodes,
    "Query.nodes",
    Vec<Option<Either<As<types::User, User>, As<types::Post, Post>>>>,
    |parents, args, ctx| {
        parents
            .iter()
            .map(|_| args.ids.iter().map(|id| node_for(ctx.app(), id)).collect())
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::search,
    "Query.search",
    Vec<Either<As<types::User, User>, As<types::Post, Post>>>,
    |parents, args, ctx| {
        let world = ctx.app();
        parents
            .iter()
            .map(|_| {
                let mut out = Vec::new();
                for u in 0..world.users {
                    if args
                        .term
                        .as_deref()
                        .is_none_or(|t| User(u).name().contains(t))
                    {
                        out.push(Either::A(As::new(User(u))));
                    }
                    for p in 0..world.posts_per_user {
                        out.push(Either::B(As::new(Post {
                            owner: u,
                            index: p,
                            tracked: false,
                        })));
                    }
                }
                out
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::matrix,
    "Query.matrix",
    Option<Vec<Vec<Option<i32>>>>,
    |parents, _args, ctx| {
        let n = ctx.app().users as i32;
        parents
            .iter()
            .map(|_| {
                Some(
                    (0..n)
                        .map(|i| {
                            (0..i)
                                .map(|j| if j % 2 == 0 { Some(j) } else { None })
                                .collect()
                        })
                        .collect(),
                )
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::echo,
    "Query.echo",
    Echo,
    |parents, args, _ctx| { parents.iter().map(|_| Echo(args.input.clone())).collect() }
);
resolver!(
    QueryRoot,
    schema::Query::json,
    "Query.json",
    Option<serde_json::Value>,
    |parents, args, _ctx| { parents.iter().map(|_| args.value.clone()).collect() }
);

fn node_for(
    world: &World,
    id: &str,
) -> Option<Either<As<types::User, User>, As<types::Post, Post>>> {
    if let Some(rest) = id.strip_prefix("post:") {
        let mut parts = rest.split(':');
        let owner: u32 = parts.next()?.parse().ok()?;
        let index: u32 = parts.next()?.parse().ok()?;
        if owner < world.users && index < world.posts_per_user {
            return Some(Either::B(As::new(Post {
                owner,
                index,
                tracked: false,
            })));
        }
        return None;
    }
    id.parse::<u32>()
        .ok()
        .filter(|&u| u < world.users)
        .map(|u| Either::A(As::new(User(u))))
}

#[derive(Clone)]
pub struct Echo(pub schema::UserPatch);

resolver!(
    QueryRoot,
    schema::Query::noisy,
    "Query.noisy",
    Streamed<futures_util::stream::BoxStream<'obj, Result<Option<i32>, Error>>>,
    |parents, args, _ctx| {
        use futures_util::StreamExt;
        let count = args.count.max(0);
        parents
            .iter()
            .map(|_| {
                Streamed::new(
                    futures_util::stream::iter(
                        (0..count).map(|i| Err(Error::new(format!("noisy item {i}")))),
                    )
                    .boxed(),
                )
            })
            .collect()
    }
);

resolver!(
    QueryRoot,
    schema::Query::stuck,
    "Query.stuck",
    Streamed<futures_util::stream::BoxStream<'obj, Result<i32, Error>>>,
    |parents, _args, _ctx| {
        use futures_util::StreamExt;
        parents
            .iter()
            .map(|_| {
                Streamed::new(
                    futures_util::stream::iter([Err(Error::new("stuck item failed"))])
                        .chain(futures_util::stream::pending())
                        .boxed(),
                )
            })
            .collect()
    }
);

resolver!(
    QueryRoot,
    schema::Query::floats,
    "Query.floats",
    Streamed<futures_util::stream::BoxStream<'obj, Result<Option<f64>, Error>>>,
    |parents, _args, _ctx| {
        use futures_util::StreamExt;
        parents
            .iter()
            .map(|_| {
                Streamed::new(
                    futures_util::stream::iter([Ok(Some(f64::NAN))])
                        .chain(futures_util::stream::pending())
                        .boxed(),
                )
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::strictFloats,
    "Query.strictFloats",
    Streamed<futures_util::stream::BoxStream<'obj, Result<f64, Error>>>,
    |parents, _args, _ctx| {
        use futures_util::StreamExt;
        parents
            .iter()
            .map(|_| {
                Streamed::new(
                    futures_util::stream::iter([Ok(f64::NAN)])
                        .chain(futures_util::stream::pending())
                        .boxed(),
                )
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::team,
    "Query.team",
    Option<Vec<User>>,
    |parents, _args, ctx| {
        let n = ctx.app().users;
        parents
            .iter()
            .map(|_| {
                Some(
                    (0..n)
                        .map(|id| User {
                            id,
                            depth: 0,
                            tracked: false,
                            hinted: false,
                        })
                        .collect(),
                )
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::strictJson,
    "Query.strictJson",
    serde_json::Value,
    |parents, args, _ctx| {
        parents
            .iter()
            .map(|_| args.value.clone().unwrap_or(serde_json::Value::Null))
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::jsons,
    "Query.jsons",
    Streamed<futures_util::stream::BoxStream<'obj, Result<Option<serde_json::Value>, Error>>>,
    |parents, _args, _ctx| {
        use futures_util::StreamExt;
        parents
            .iter()
            .map(|_| {
                Streamed::new(
                    futures_util::stream::iter([Ok(Some(serde_json::Value::Null))])
                        .chain(futures_util::stream::pending())
                        .boxed(),
                )
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::strictJsons,
    "Query.strictJsons",
    Streamed<futures_util::stream::BoxStream<'obj, Result<serde_json::Value, Error>>>,
    |parents, _args, _ctx| {
        use futures_util::StreamExt;
        parents
            .iter()
            .map(|_| {
                Streamed::new(
                    futures_util::stream::iter([Ok(serde_json::Value::Null)])
                        .chain(futures_util::stream::pending())
                        .boxed(),
                )
            })
            .collect()
    }
);
resolver!(
    QueryRoot,
    schema::Query::count,
    "Query.count",
    i32,
    |parents, args, _ctx| {
        parents
            .iter()
            .map(|_| (args.bag.values.len() + args.bag.blobs.len()) as i32)
            .collect()
    }
);

fn not_chain(filter: &schema::Filter) -> i32 {
    1 + filter.not.as_ref().map_or(0, |next| not_chain(next))
}

resolver!(
    QueryRoot,
    schema::Query::nest,
    "Query.nest",
    i32,
    |parents, args, _ctx| { parents.iter().map(|_| not_chain(&args.filter)).collect() }
);
resolver!(
    QueryRoot,
    schema::Query::numbers,
    "Query.numbers",
    Streamed<futures_util::stream::BoxStream<'obj, Result<Option<i32>, Error>>>,
    |parents, args, ctx| {
        use futures_util::StreamExt;
        let fail = args.fail;
        let panics = ctx.app().panic_after_stream_error;
        parents
            .iter()
            .map(|_| {
                let items = futures_util::stream::iter((0..3i32).map(move |i| {
                    if fail == Some(i) {
                        Err(Error::new(format!("number {i} failed")))
                    } else {
                        Ok(Some(i + 1))
                    }
                }));
                if panics {
                    let end = fail.map_or(3, |f| f + 1) as usize;
                    Streamed::new(
                        items
                            .take(end)
                            .chain(futures_util::stream::poll_fn(
                                |_| -> std::task::Poll<Option<Result<Option<i32>, Error>>> {
                                    panic!("pulled past the failing item")
                                },
                            ))
                            .boxed(),
                    )
                } else {
                    Streamed::new(items.boxed())
                }
            })
            .collect()
    }
);

resolver!(User, schema::User::id, "User.id", Result<String, Error>, |parents, _args, ctx| {
    parents.iter().map(|u| fail_or(ctx.app(), "User", "id", u.id, u.id.to_string())).collect()
});
resolver!(
    User,
    schema::User::uuid,
    "User.uuid",
    uuid::Uuid,
    |parents, _args, _ctx| { parents.iter().map(|u| uuid_for(u.id)).collect() }
);
resolver!(User, schema::User::name, "User.name", Result<String, Error>, |parents, _args, ctx| {
    parents.iter().map(|u| fail_or(ctx.app(), "User", "name", u.id, u.name())).collect()
});
resolver!(
    User,
    schema::User::email,
    "User.email",
    Result<Option<String>, Error>,
    |parents, _args, ctx| {
        parents
            .iter()
            .map(|u| {
                fail_or(
                    ctx.app(),
                    "User",
                    "email",
                    u.id,
                    (u.id % 2 == 0).then(|| format!("user{}@example.com", u.id)),
                )
            })
            .collect()
    }
);
resolver!(
    User,
    schema::User::role,
    "User.role",
    schema::Role,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|u| {
                if u.id == 0 {
                    schema::Role::ADMIN
                } else {
                    schema::Role::MEMBER
                }
            })
            .collect()
    }
);
resolver!(
    User,
    schema::User::posts,
    "User.posts",
    Result<Box<[Post]>, Error>,
    |parents, args, ctx| {
        let world = ctx.app();
        parents
            .iter()
            .map(|u| {
                fail_or(
                    world,
                    "User",
                    "posts",
                    u.id,
                    (0..world
                        .posts_per_user
                        .min(args.first.unwrap_or(10).max(0) as u32))
                        .map(|p| Post {
                            owner: u.id,
                            index: p,
                            tracked: ctx.app().track_drops,
                        })
                        .collect(),
                )
            })
            .collect()
    }
);
resolver!(
    User,
    schema::User::drafts,
    "User.drafts",
    Streamed<futures_util::stream::BoxStream<'obj, Result<Post, Error>>>,
    |parents, _args, ctx| {
        use futures_util::StreamExt;
        let world = ctx.app();
        parents
            .iter()
            .map(|u| {
                let owner = u.id;
                let n = world.posts_per_user;
                let failing = world.fails("User", "drafts", owner);
                let tracked = world.track_drops;
                if world.stalled_drafts == Some(owner) {
                    return Streamed::new(futures_util::stream::pending().boxed());
                }
                if world.panic_drafts == Some(owner) {
                    return Streamed::new(
                        futures_util::stream::poll_fn(
                            move |_| -> std::task::Poll<Option<Result<Post, Error>>> {
                                panic!("drafts of {owner} were pulled after a halt")
                            },
                        )
                        .boxed(),
                    );
                }
                Streamed::new(
                    futures_util::stream::iter((0..n).map(move |p| {
                        if failing && p == 1 {
                            Err(Error::new(format!("User.drafts failed for {owner} at {p}")))
                        } else {
                            Ok(Post {
                                owner,
                                index: p,
                                tracked,
                            })
                        }
                    }))
                    .boxed(),
                )
            })
            .collect()
    }
);
resolver!(
    User,
    schema::User::friends,
    "User.friends",
    Vec<Option<User>>,
    |parents, _args, ctx| {
        let n = ctx.app().users;
        let tracked = ctx.app().track_drops;
        parents
            .iter()
            .map(|u| {
                (0..n)
                    .map(|f| {
                        if f == u.id {
                            None
                        } else {
                            Some(User {
                                id: f,
                                depth: u.depth + 1,
                                tracked,
                                hinted: false,
                            })
                        }
                    })
                    .collect()
            })
            .collect()
    }
);
resolver!(
    User,
    schema::User::score,
    "User.score",
    Option<f64>,
    |parents, _args, ctx| {
        parents
            .iter()
            .map(|u| {
                if ctx.app().nan_score {
                    Some(f64::NAN)
                } else if u.id % 3 == 0 {
                    None
                } else {
                    Some(u.id as f64 * 1.5)
                }
            })
            .collect()
    }
);

resolver!(
    Post,
    schema::Post::id,
    "Post.id",
    String,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|p| format!("post:{}:{}", p.owner, p.index))
            .collect()
    }
);
resolver!(Post, schema::Post::title, "Post.title", Result<String, Error>, |parents, _args, ctx| {
    parents.iter().map(|p| fail_or(ctx.app(), "Post", "title", p.owner * 100 + p.index, format!("post {} of user{}", p.index, p.owner))).collect()
});
resolver!(
    Post,
    schema::Post::owner,
    "Post.owner",
    Option<User>,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|p| (p.index % 2 == 0).then(|| User(p.owner)))
            .collect()
    }
);
resolver!(Post, schema::Post::author, "Post.author", Result<User, Error>, |parents, _args, ctx| {
    parents.iter().map(|p| fail_or(ctx.app(), "Post", "author", p.owner * 100 + p.index, User(p.owner))).collect()
});
resolver!(
    Post,
    schema::Post::tags,
    "Post.tags",
    std::sync::Arc<[String]>,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|p| (0..p.index + 1).map(|t| format!("t{t}")).collect())
            .collect()
    }
);

resolver!(
    Echo,
    schema::UserPatchOut::name,
    "UserPatchOut.name",
    Option<String>,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|e| match &e.0.name {
                greem::Maybe::Value(v) => Some(v.clone()),
                greem::Maybe::Null => Some("<null>".into()),
                greem::Maybe::Absent => None,
            })
            .collect()
    }
);
resolver!(
    Echo,
    schema::UserPatchOut::email,
    "UserPatchOut.email",
    Option<String>,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|e| match &e.0.email {
                greem::Maybe::Value(v) => Some(v.clone()),
                greem::Maybe::Null => Some("<null>".into()),
                greem::Maybe::Absent => None,
            })
            .collect()
    }
);
resolver!(
    Echo,
    schema::UserPatchOut::role,
    "UserPatchOut.role",
    schema::Role,
    |parents, _args, _ctx| {
        parents
            .iter()
            .map(|e| e.0.role.unwrap_or(schema::Role::MEMBER))
            .collect()
    }
);

resolver!(
    MutationRoot,
    schema::Mutation::rename,
    "Mutation.rename",
    Option<User>,
    |parents, args, ctx| {
        parents
            .iter()
            .map(|_| {
                args.id
                    .parse::<u32>()
                    .ok()
                    .filter(|&id| id < ctx.app().users)
                    .map(User)
            })
            .collect()
    }
);
resolver!(
    MutationRoot,
    schema::Mutation::patch,
    "Mutation.patch",
    Option<User>,
    |parents, args, ctx| {
        parents
            .iter()
            .map(|_| {
                args.id
                    .parse::<u32>()
                    .ok()
                    .filter(|&id| id < ctx.app().users)
                    .map(User)
            })
            .collect()
    }
);
resolver!(MutationRoot, schema::Mutation::fail, "Mutation.fail", Result<String, Error>, |parents, _args, _ctx| {
    parents.iter().map(|_| Err(Error::new("Mutation.fail failed"))).collect()
});

resolver!(
    MutationRoot,
    schema::Mutation::stamp,
    "Mutation.stamp",
    uuid::Uuid,
    |parents, args, _ctx| { parents.iter().map(|_| args.id).collect() }
);

pub type Schema = schema::Schema<World, QueryRoot, MutationRoot>;

pub fn build_schema() -> Schema {
    schema::Schema::<World>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .build()
        .expect("property schema builds")
}
