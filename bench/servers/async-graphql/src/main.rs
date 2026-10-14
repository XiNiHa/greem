//! The async-graphql implementation of the resolver contract
//! (`../../workloads/CONTRACT.md`), served with `async-graphql-axum` the way
//! its book shows: `#[Object]` resolvers, per-request data from a header.

use async_graphql::{
    Context, EmptyMutation, EmptySubscription, ID, Interface, Object, Schema, Union,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const POSTS_PER_USER: u32 = 3;

/// The fetch latency of the workload a request names.
#[derive(Clone, Default)]
struct Latency(Arc<HashMap<String, Duration>>);

async fn fetch(ctx: &Context<'_>, field: &str) {
    if let Some(latency) = ctx.data_unchecked::<Latency>().0.get(field) {
        tokio::time::sleep(*latency).await;
    }
}

fn count(n: i32) -> u32 {
    n.max(0) as u32
}

fn posts_of(owner: u32) -> impl Iterator<Item = Post> {
    (0..POSTS_PER_USER).map(move |index| Post { owner, index })
}

struct Query;

#[Object]
impl Query {
    async fn users(&self, ctx: &Context<'_>, first: i32) -> Vec<User> {
        fetch(ctx, "Query.users").await;
        (0..count(first)).map(|id| User { id }).collect()
    }

    async fn search(&self, ctx: &Context<'_>, size: i32) -> Vec<SearchResult> {
        fetch(ctx, "Query.search").await;
        (0..count(size))
            .map(|j| {
                if j % 2 == 0 {
                    SearchResult::User(User { id: j })
                } else {
                    SearchResult::Post(Post { owner: j, index: 0 })
                }
            })
            .collect()
    }

    async fn matrix(&self, ctx: &Context<'_>, size: i32) -> Option<Vec<Vec<Option<i32>>>> {
        fetch(ctx, "Query.matrix").await;
        let size = count(size) as i32;
        Some(
            (0..size)
                .map(|i| {
                    (0..i)
                        .map(|j| if j % 2 == 0 { Some(j) } else { None })
                        .collect()
                })
                .collect(),
        )
    }
}

struct User {
    id: u32,
}

#[Object]
impl User {
    async fn id(&self) -> ID {
        ID(self.id.to_string())
    }

    async fn name(&self) -> String {
        format!("user{}", self.id)
    }

    async fn email(&self) -> Option<String> {
        self.id
            .is_multiple_of(2)
            .then(|| format!("user{}@example.com", self.id))
    }

    async fn posts(&self, ctx: &Context<'_>) -> Vec<Post> {
        fetch(ctx, "User.posts").await;
        posts_of(self.id).collect()
    }

    /// The same posts; async-graphql has no lazy list output, so a `Vec` is
    /// its idiom.
    async fn drafts(&self, ctx: &Context<'_>) -> Vec<Post> {
        fetch(ctx, "User.drafts").await;
        posts_of(self.id).collect()
    }

    async fn friends(&self, ctx: &Context<'_>) -> Vec<User> {
        fetch(ctx, "User.friends").await;
        vec![User { id: self.id + 1 }, User { id: self.id + 2 }]
    }
}

struct Post {
    owner: u32,
    index: u32,
}

#[Object]
impl Post {
    async fn id(&self) -> ID {
        ID(format!("{}-{}", self.owner, self.index))
    }

    async fn title(&self) -> String {
        format!("Post {} by user{}", self.index, self.owner)
    }

    async fn tags(&self) -> Vec<String> {
        vec![format!("t{}", self.index), format!("u{}", self.owner)]
    }
}

#[derive(Union)]
enum SearchResult {
    User(User),
    Post(Post),
}

#[derive(Interface)]
#[graphql(field(name = "id", ty = "ID"))]
enum Node {
    User(User),
    Post(Post),
}

type BenchSchema = Schema<Query, EmptyMutation, EmptySubscription>;

#[derive(Clone)]
struct AppState {
    schema: BenchSchema,
    latencies: Arc<HashMap<String, Latency>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(default)]
    latency_ms: HashMap<String, u64>,
}

/// Every workload's fetch latency by name, from `WORKLOADS`.
fn latencies(dir: &Path) -> HashMap<String, Latency> {
    std::fs::read_dir(dir)
        .expect("workloads directory")
        .filter_map(|entry| {
            let entry = entry.expect("directory entry");
            let manifest = entry.path().join("workload.json");
            let text = std::fs::read_to_string(&manifest).ok()?;
            let manifest: Manifest = serde_json::from_str(&text).expect("workload.json");
            let latency = manifest
                .latency_ms
                .into_iter()
                .map(|(field, ms)| (field, Duration::from_millis(ms)))
                .collect();
            Some((
                entry.file_name().to_string_lossy().into_owned(),
                Latency(Arc::new(latency)),
            ))
        })
        .collect()
}

async fn graphql(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let latency = headers
        .get("x-workload")
        .and_then(|v| v.to_str().ok())
        .and_then(|name| state.latencies.get(name))
        .cloned()
        .unwrap_or_default();
    state
        .schema
        .execute(request.into_inner().data(latency))
        .await
        .into()
}

#[tokio::main]
async fn main() {
    let workloads = std::env::var("WORKLOADS")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../workloads").to_owned());
    let port: u16 = std::env::var("PORT")
        .ok()
        .map(|p| p.parse().expect("PORT is a port number"))
        .unwrap_or(4002);
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
        .register_output_type::<Node>()
        .finish();
    let state = AppState {
        schema,
        latencies: Arc::new(latencies(Path::new(&workloads))),
    };
    let app = Router::new()
        .route("/graphql", post(graphql))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    println!("async-graphql bench server listening on http://127.0.0.1:{port}/graphql");
    axum::serve(listener, app).await.expect("serve");
}
