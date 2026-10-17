//! The greem implementation of the resolver contract (`bench/workloads/CONTRACT.md`):
//! every value a function of indices, fetch fields set-based and paying
//! their declared latency once per call, leaves through the per-object sugar.

use crate::schema;
use futures::StreamExt;
use greem::{Args, Context, Error, Streamed};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// The application context: only the fetch latency a workload declared.
#[derive(Clone, Debug, Default)]
pub struct App {
    latency: Arc<BTreeMap<String, Duration>>,
}

impl App {
    pub fn new(latency: BTreeMap<String, Duration>) -> Self {
        App {
            latency: Arc::new(latency),
        }
    }

    async fn fetch(&self, field: &str) {
        if let Some(latency) = self.latency.get(field) {
            tokio::time::sleep(*latency).await;
        }
    }
}

pub struct QueryRoot;

pub struct User {
    pub id: u32,
}

pub struct Post {
    pub owner: u32,
    pub index: u32,
}

#[derive(greem::Abstract)]
#[greem(type = "SearchResult")]
pub enum Hit {
    User(User),
    Post(Post),
}

const POSTS_PER_USER: u32 = 3;

fn count(n: i32) -> u32 {
    n.max(0) as u32
}

fn posts_of(owner: u32) -> impl Iterator<Item = Post> {
    (0..POSTS_PER_USER).map(move |index| Post { owner, index })
}

#[greem::object(context = App, type = "Query")]
impl QueryRoot {
    async fn users(
        parents: &[&Self],
        args: &Args<schema::Query::users>,
        ctx: &Context<App>,
    ) -> Vec<Vec<User>> {
        ctx.app().fetch("Query.users").await;
        let first = count(args.first);
        parents
            .iter()
            .map(|_| (0..first).map(|id| User { id }).collect())
            .collect()
    }

    async fn search(
        parents: &[&Self],
        args: &Args<schema::Query::search>,
        ctx: &Context<App>,
    ) -> Vec<Vec<Hit>> {
        ctx.app().fetch("Query.search").await;
        let size = count(args.size);
        parents
            .iter()
            .map(|_| {
                (0..size)
                    .map(|j| {
                        if j.is_multiple_of(2) {
                            Hit::User(User { id: j })
                        } else {
                            Hit::Post(Post { owner: j, index: 0 })
                        }
                    })
                    .collect()
            })
            .collect()
    }

    async fn matrix(
        parents: &[&Self],
        args: &Args<schema::Query::matrix>,
        ctx: &Context<App>,
    ) -> Vec<Option<Vec<Vec<Option<i32>>>>> {
        ctx.app().fetch("Query.matrix").await;
        let size = count(args.size) as i32;
        parents
            .iter()
            .map(|_| {
                Some(
                    (0..size)
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
}

#[greem::object(context = App)]
impl User {
    fn id(&self) -> String {
        self.id.to_string()
    }

    fn name(&self) -> String {
        format!("user{}", self.id)
    }

    fn email(&self) -> Option<String> {
        self.id
            .is_multiple_of(2)
            .then(|| format!("user{}@example.com", self.id))
    }

    async fn posts(
        parents: &[&Self],
        _args: &Args<schema::User::posts>,
        ctx: &Context<App>,
    ) -> Vec<Vec<Post>> {
        ctx.app().fetch("User.posts").await;
        parents.iter().map(|u| posts_of(u.id).collect()).collect()
    }

    async fn drafts(
        parents: &[&Self],
        _args: &Args<schema::User::drafts>,
        ctx: &Context<App>,
    ) -> Vec<Streamed<futures::stream::BoxStream<'static, Result<Post, Error>>>> {
        ctx.app().fetch("User.drafts").await;
        parents
            .iter()
            .map(|u| Streamed::new(futures::stream::iter(posts_of(u.id).map(Ok)).boxed()))
            .collect()
    }

    async fn friends(
        parents: &[&Self],
        _args: &Args<schema::User::friends>,
        ctx: &Context<App>,
    ) -> Vec<Vec<User>> {
        ctx.app().fetch("User.friends").await;
        parents
            .iter()
            .map(|u| vec![User { id: u.id + 1 }, User { id: u.id + 2 }])
            .collect()
    }
}

#[greem::object(context = App)]
impl Post {
    fn id(&self) -> String {
        format!("{}-{}", self.owner, self.index)
    }

    fn title(&self) -> String {
        format!("Post {} by user{}", self.index, self.owner)
    }

    fn tags(&self) -> Vec<String> {
        vec![format!("t{}", self.index), format!("u{}", self.owner)]
    }
}

pub type Schema = schema::Schema<App, QueryRoot>;
pub type Plan = schema::Plan<App, QueryRoot>;

pub fn build_schema() -> Schema {
    schema::Schema::<App>::builder()
        .query::<QueryRoot>()
        .build()
        .expect("bench schema builds")
}
