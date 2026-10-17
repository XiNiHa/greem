//! The walking skeleton's example server: schema compilation from `build.rs`,
//! per-field resolvers (set-based and per-object sugar), a member enum at an
//! interface position, lookbehind hints, and an axum handler that hands each
//! request to `greem::http::handle`, which streams `@defer`/`@stream`
//! payloads as the execution ships them.

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use futures::{StreamExt, stream};
use greem::{
    Args, Context, Error, ErrorBehavior, ExecuteOptions, IncrementalDelivery, Planning, Resolver,
    Roots, Streamed,
};
use std::sync::{Arc, Mutex};

mod schema {
    greem::include_schema!();
}

/// The application context: the "database" plus a fetch log the tests read.
/// Rows sit behind `Arc` the way a cache hands them out, and resolvers return
/// those `Arc`s as objects; leaf outputs borrow from the rows.
pub struct App {
    users: Vec<Arc<User>>,
    posts: Vec<Arc<Post>>,
    log: Mutex<Vec<String>>,
}

#[derive(Clone)]
pub struct User {
    id: u32,
    uuid: uuid::Uuid,
    name: String,
    email: Option<String>,
}

pub struct Post {
    id: u32,
    author: u32,
    title: String,
}

/// A member enum named after the interface it outputs, so `type` is implied.
#[derive(greem::Abstract)]
pub enum Node {
    User(Arc<User>),
    Post(Arc<Post>),
}

impl App {
    fn seeded() -> Self {
        App {
            users: vec![
                Arc::new(User {
                    id: 1,
                    uuid: uuid::Uuid::from_u128(1),
                    name: "Ann".into(),
                    email: Some("ann@example.com".into()),
                }),
                Arc::new(User {
                    id: 2,
                    uuid: uuid::Uuid::from_u128(2),
                    name: "Bob".into(),
                    email: None,
                }),
            ],
            posts: vec![
                Arc::new(Post {
                    id: 10,
                    author: 1,
                    title: "Hello".into(),
                }),
                Arc::new(Post {
                    id: 11,
                    author: 1,
                    title: "Again".into(),
                }),
                Arc::new(Post {
                    id: 20,
                    author: 2,
                    title: "Hi".into(),
                }),
            ],
            log: Mutex::new(Vec::new()),
        }
    }

    fn log(&self, line: impl Into<String>) {
        self.log.lock().unwrap().push(line.into());
    }
}

/// A hint `Post.author` leaves for whichever `posts` field sits above it.
#[derive(Default, Debug)]
pub struct PostsHint {
    pub with_author: bool,
}

pub struct QueryRoot;
pub struct MutationRoot;

// ---- Query: hand-written set-based resolvers -------------------------------

impl Resolver<schema::Query::users, App> for QueryRoot {
    type Output<'obj>
        = Vec<Arc<User>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log("Query.users");
        Ok(parents.iter().map(|_| ctx.app().users.clone()).collect())
    }
}

impl Resolver<schema::Query::user, App> for QueryRoot {
    type Output<'obj>
        = Option<Arc<User>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Query::user>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("Query.user({})", args.id));
        Ok(parents
            .iter()
            .map(|_| {
                ctx.app()
                    .users
                    .iter()
                    .find(|u| u.id.to_string() == args.id)
                    .cloned()
            })
            .collect())
    }
}

impl Resolver<schema::Query::node, App> for QueryRoot {
    type Output<'obj>
        = Option<Node>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Query::node>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        let app = ctx.app();
        Ok(parents
            .iter()
            .map(|_| {
                if let Some(user) = app.users.iter().find(|u| u.id.to_string() == args.id) {
                    return Some(Node::User(user.clone()));
                }
                app.posts
                    .iter()
                    .find(|p| p.id.to_string() == args.id)
                    .map(|p| Node::Post(p.clone()))
            })
            .collect())
    }
}

impl Resolver<schema::Query::posts, App> for QueryRoot {
    type Output<'obj>
        = Streamed<futures::stream::BoxStream<'obj, Result<Arc<Post>, Error>>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Query::posts>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        let hint = ctx.hint::<PostsHint>();
        ctx.app()
            .log(format!("Query.posts(with_author: {})", hint.with_author));
        let first = args.first.unwrap_or(10).max(0) as usize;
        Ok(parents
            .iter()
            .map(|_| {
                Streamed::new(
                    stream::iter(ctx.app().posts.iter().take(first).map(|p| Ok(p.clone()))).boxed(),
                )
            })
            .collect())
    }

    fn hints(registry: &mut greem::HintRegistry<'_>) {
        registry.accept::<PostsHint>();
    }
}

// ---- User: the per-object sugar ---------------------------------------------

#[greem::object(context = App)]
impl User {
    async fn id(&self) -> String {
        self.id.to_string()
    }

    fn uuid(&self) -> &uuid::Uuid {
        &self.uuid
    }

    async fn name(&self) -> &str {
        &self.name
    }

    fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    /// Set-based: one call per scope, hinted by `Post.author`.
    fn posts(
        parents: &[&Self],
        _args: &Args<schema::User::posts>,
        ctx: &Context<App>,
    ) -> Vec<Vec<Arc<Post>>> {
        let with_author = ctx.hint::<PostsHint>().with_author;
        ctx.app().log(format!(
            "User.posts x{} (with_author: {with_author})",
            parents.len()
        ));
        parents
            .iter()
            .map(|u| {
                ctx.app()
                    .posts
                    .iter()
                    .filter(|p| p.author == u.id)
                    .cloned()
                    .collect()
            })
            .collect()
    }

    #[greem(hints = "posts")]
    fn posts_hints(registry: &mut greem::HintRegistry<'_>) {
        registry.accept::<PostsHint>();
    }
}

// ---- Post -------------------------------------------------------------------

#[greem::object(context = App)]
impl Post {
    fn id(&self) -> String {
        self.id.to_string()
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn author(
        &self,
        _args: &Args<schema::Post::author>,
        ctx: &Context<App>,
    ) -> Result<Arc<User>, Error> {
        ctx.app().log("Post.author");
        ctx.app()
            .users
            .iter()
            .find(|u| u.id == self.author)
            .cloned()
            .ok_or_else(|| Error::new("author missing"))
    }

    #[greem(plan = "author")]
    fn author_plan(planning: &mut Planning<'_, schema::Post::author, App>) {
        planning.hint::<PostsHint>(|h| h.with_author = true);
    }
}

// ---- Mutation ----------------------------------------------------------------

impl Resolver<schema::Mutation::rename, App> for MutationRoot {
    type Output<'obj>
        = Option<Arc<User>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Mutation::rename>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        let app = ctx.app();
        Ok(parents
            .iter()
            .map(|_| {
                app.users
                    .iter()
                    .find(|u| u.id.to_string() == args.id)
                    .map(|u| {
                        Arc::new(User {
                            name: args.name.clone(),
                            ..User::clone(u)
                        })
                    })
            })
            .collect())
    }
}

// ---- HTTP ----------------------------------------------------------------------

type Schema = schema::Schema<App, QueryRoot, MutationRoot>;

pub fn build_schema() -> Schema {
    schema::Schema::<App>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .build()
        .expect("schema builds")
}

pub fn router(schema: Schema) -> Router {
    Router::new()
        .route("/graphql", any(graphql))
        .with_state(schema)
}

async fn graphql(State(schema): State<Schema>, parts: Parts, body: Bytes) -> Response {
    let roots = Roots {
        query: QueryRoot,
        mutation: MutationRoot,
    };
    let options = ExecuteOptions {
        default_error_behavior: ErrorBehavior::Propagate,
        incremental: IncrementalDelivery::Enabled,
    };
    let request = greem::http::RequestParts::from_http(&parts, &body);
    let response = greem::http::handle(&schema, roots, App::seeded(), request, options).await;
    axum::http::Response::from(response).into_response()
}

#[tokio::main]
async fn main() {
    let schema = build_schema();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("bind");
    println!("greem example listening on http://127.0.0.1:8080/graphql");
    axum::serve(listener, router(schema)).await.expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{StatusCode, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn send(router: Router, accept: &str, body: serde_json::Value) -> Response {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/graphql")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, accept)
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        router.oneshot(request).await.unwrap()
    }

    async fn post(
        router: Router,
        accept: &str,
        body: serde_json::Value,
    ) -> (StatusCode, String, String) {
        let response = send(router, accept, body).await;
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            content_type,
            String::from_utf8(bytes.to_vec()).unwrap(),
        )
    }

    #[tokio::test]
    async fn query_end_to_end() {
        let router = router(build_schema());
        let (status, content_type, body) = post(
            router,
            "application/json",
            serde_json::json!({"query": "{ users { id uuid name email posts { title author { name } } } node(id: \"10\") { id ... on Post { title } } }"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "application/json; charset=utf-8");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["data"]["users"][0]["posts"][1]["author"]["name"], "Ann");
        assert_eq!(
            v["data"]["users"][0]["uuid"],
            "00000000-0000-0000-0000-000000000001"
        );
        assert_eq!(v["data"]["users"][1]["email"], serde_json::Value::Null);
        assert_eq!(
            v["data"]["node"],
            serde_json::json!({"id": "10", "title": "Hello"})
        );
    }

    #[tokio::test]
    async fn defer_and_stream_over_multipart() {
        let router = router(build_schema());
        let (status, content_type, body) = post(
            router,
            "multipart/mixed;incrementalSpec=v0.2,application/graphql-response+json,application/json;q=0.9",
            serde_json::json!({"query": "{ users { name ... @defer { posts { title } } } posts(first: 3) @stream(initialCount: 1) { title } }"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            content_type,
            "multipart/mixed; boundary=\"-\"; incrementalSpec=v0.2"
        );
        let parts: Vec<&str> = body
            .split("\r\n---\r\n")
            .filter(|p| !p.is_empty() && !p.starts_with("--"))
            .collect();
        let payloads: Vec<serde_json::Value> = parts
            .iter()
            .map(|part| {
                let json = part
                    .split("\r\n\r\n")
                    .nth(1)
                    .unwrap()
                    .trim_end_matches("\r\n-----\r\n");
                serde_json::from_str(json).unwrap()
            })
            .collect();
        assert_eq!(
            payloads[0]["data"]["users"],
            serde_json::json!([{"name": "Ann"}, {"name": "Bob"}])
        );
        assert_eq!(
            payloads[0]["data"]["posts"],
            serde_json::json!([{"title": "Hello"}])
        );
        assert_eq!(payloads[0]["hasNext"], true);
        let last = payloads.last().unwrap();
        assert_eq!(last["hasNext"], false);
        let items: Vec<serde_json::Value> = payloads[1..]
            .iter()
            .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
            .flat_map(|e| e["items"].as_array().cloned().unwrap_or_default())
            .collect();
        assert_eq!(
            items,
            serde_json::json!([{"title": "Again"}, {"title": "Hi"}])
                .as_array()
                .unwrap()
                .clone()
        );
        let deferred: Vec<serde_json::Value> = payloads[1..]
            .iter()
            .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
            .filter(|e| e.get("data").is_some())
            .collect();
        assert_eq!(
            deferred[0]["data"],
            serde_json::json!({"posts": [{"title": "Hello"}, {"title": "Again"}]})
        );
    }

    #[tokio::test]
    async fn defer_spec_only_clients_get_one_json_response() {
        let router = router(build_schema());
        let (status, content_type, body) = post(
            router,
            "multipart/mixed;deferSpec=20220824,application/json",
            serde_json::json!({"query": "{ users { name ... @defer { posts { title } } } posts(first: 3) @stream(initialCount: 1) { title } }"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "application/json; charset=utf-8");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v.get("hasNext").is_none(), "{v}");
        assert_eq!(
            v["data"]["users"][0],
            serde_json::json!({"name": "Ann", "posts": [{"title": "Hello"}, {"title": "Again"}]})
        );
        assert_eq!(
            v["data"]["posts"],
            serde_json::json!([{"title": "Hello"}, {"title": "Again"}, {"title": "Hi"}])
        );
    }

    #[tokio::test]
    async fn each_payload_is_its_own_delimited_chunk() {
        let router = router(build_schema());
        let response = send(
            router,
            "multipart/mixed",
            serde_json::json!({"query": "{ users { name ... @defer { posts { title } } } posts(first: 3) @stream(initialCount: 1) { title } }"}),
        )
        .await;
        let mut body = response.into_body();
        let mut chunks = Vec::new();
        while let Some(frame) = body.frame().await {
            let data = frame.unwrap().into_data().unwrap();
            chunks.push(String::from_utf8(data.to_vec()).unwrap());
        }
        let (last, rest) = chunks.split_last().unwrap();
        assert!(!rest.is_empty());
        assert!(rest[0].starts_with("\r\n---\r\n"), "{:?}", rest[0]);
        for chunk in rest {
            assert!(chunk.ends_with("\r\n---\r\n"), "{chunk:?}");
            assert!(chunk.contains("\"hasNext\":true"), "{chunk:?}");
        }
        assert!(last.ends_with("\r\n-----\r\n"), "{last:?}");
        assert!(last.contains("\"hasNext\":false"), "{last:?}");
    }

    #[tokio::test]
    async fn hints_reach_the_accepting_field_through_the_sugar() {
        let schema = build_schema();
        let document = schema
            .parse("{ users { posts { author { name } } } posts { author { name } } }")
            .unwrap();
        let app = App::seeded();
        let output = schema
            .execute(
                Roots {
                    query: QueryRoot,
                    mutation: MutationRoot,
                },
                app,
                greem::Operation::new(document),
                ExecuteOptions::default(),
            )
            .await;
        let v: serde_json::Value = serde_json::from_slice(&output.first().json).unwrap();
        assert_eq!(v["data"]["posts"][2]["author"]["name"], "Bob");
    }

    #[tokio::test]
    async fn validation_failures_are_unprocessable() {
        let router = router(build_schema());
        let (status, content_type, body) = post(
            router,
            "application/graphql-response+json, application/json;q=0.9",
            serde_json::json!({"query": "{ nope }"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            content_type,
            "application/graphql-response+json; charset=utf-8"
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v.get("data").is_none());
        assert!(v["errors"][0]["message"].as_str().unwrap().contains("nope"));
    }

    #[tokio::test]
    async fn queries_also_arrive_over_get() {
        let request =
            axum::http::Request::get("/graphql?query=%7B+user(id%3A+%222%22)+%7B+name+%7D+%7D")
                .header(header::ACCEPT, "application/graphql-response+json")
                .body(Body::empty())
                .unwrap();
        let response = router(build_schema()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, serde_json::json!({"data": {"user": {"name": "Bob"}}}));
    }

    #[tokio::test]
    async fn queries_also_arrive_over_query() {
        let request = axum::http::Request::builder()
            .method("QUERY")
            .uri("/graphql")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/graphql-response+json")
            .body(Body::from(r#"{"query": "{ user(id: \"2\") { name } }"}"#))
            .unwrap();
        let response = router(build_schema()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, serde_json::json!({"data": {"user": {"name": "Bob"}}}));
    }
}
