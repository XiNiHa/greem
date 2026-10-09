//! The runtime end to end, over `greem-test-app`'s generated schema module
//! and hand-written resolvers.

use futures::StreamExt;
use futures::executor::block_on;
use greem::__private::ToLeaf;
use greem::{
    Args, As, Context, Either, Error, ErrorBehavior, ExecuteOptions, IncrementalDelivery, Items,
    Operation, Resolver, Roots, Streamed,
};
use greem_test_app::app::{App, MutationRoot, QueryRoot, User, build_schema, users};
use greem_test_app::schema;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn run(
    app: App,
    query: &str,
    variables: Value,
    options: ExecuteOptions,
) -> (Vec<Value>, Vec<String>) {
    let schema = build_schema();
    let document = schema.parse(query).unwrap_or_else(|e| panic!("{e:?}"));
    let output = block_on(schema.execute(
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        app,
        Operation {
            document: document.clone(),
            operation_name: None,
            variables,
        },
        options,
    ));
    let payloads = output
        .payloads
        .iter()
        .map(|p| serde_json::from_slice(&p.json).unwrap())
        .collect();
    (payloads, Vec::new())
}

fn single(app: App, query: &str) -> Value {
    let (payloads, _) = run(app, query, Value::Null, ExecuteOptions::default());
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    payloads.into_iter().next().unwrap()
}

#[test]
fn flat_and_nested() {
    let v = single(
        App::default(),
        "{ users { id name email posts(first: 1) { title author { name } } } }",
    );
    assert_eq!(
        v,
        json!({"data": {"users": [
            {"id": "1", "name": "Ann", "email": "ann@x", "posts": [{"title": "Ann post 0", "author": {"name": "Ann"}}]},
            {"id": "2", "name": "Bob", "email": null, "posts": [{"title": "Bob post 0", "author": {"name": "Bob"}}]}
        ]}})
    );
}

#[test]
fn set_based_calls_once_per_generation() {
    let schema = build_schema();
    let document = schema
        .parse("{ users { posts { title author { name } } } }")
        .unwrap();
    let output = block_on(schema.execute(
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        App {
            log: Mutex::new(Vec::new()),
            ..Default::default()
        },
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    assert_eq!(
        v["data"]["users"][1]["posts"][1]["author"]["name"],
        json!("Bob")
    );
}

#[test]
fn nullable_propagation() {
    let v = single(
        App {
            fail_name: true,
            ..Default::default()
        },
        "{ users { name } user(id: \"2\") { name } }",
    );
    // users: [User!]! -> a non-null name failure nulls the item, then the list, then data.
    assert_eq!(v["data"], Value::Null);
    let errors = v["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["path"], json!(["users", 1, "name"]));
    assert_eq!(errors[1]["path"], json!(["user", "name"]));
}

#[test]
fn nullable_absorbs() {
    let v = single(
        App {
            fail_email: true,
            ..Default::default()
        },
        "{ users { name email } }",
    );
    assert_eq!(v["data"]["users"][0]["email"], Value::Null);
    assert_eq!(v["data"]["users"][0]["name"], json!("Ann"));
    assert_eq!(v["errors"][0]["message"], json!("email failed"));
    assert_eq!(v["errors"][0]["path"], json!(["users", 0, "email"]));
}

#[test]
fn error_behavior_null_and_halt() {
    let (p, _) = run(
        App {
            fail_name: true,
            ..Default::default()
        },
        "{ users { name } }",
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"]["users"][1]["name"], Value::Null);
    assert_eq!(p[0]["data"]["users"][0]["name"], json!("Ann"));
    let (p, _) = run(
        App {
            fail_name: true,
            ..Default::default()
        },
        "{ users { name email } }",
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], Value::Null);
    assert_eq!(p[0]["errors"].as_array().unwrap().len(), 1);
}

#[test]
fn interfaces_and_unions() {
    let v = single(
        App::default(),
        r#"{ a: node(id: "1") { id ... on User { name } ... on Post { title } } b: node(id: "p1") { __typename id ... on Post { title } } search { __typename ... on User { name } ... on Post { title } } }"#,
    );
    assert_eq!(v["data"]["a"], json!({"id": "1", "name": "Ann"}));
    assert_eq!(
        v["data"]["b"],
        json!({"__typename": "Post", "id": "10", "title": "Hello"})
    );
    assert_eq!(
        v["data"]["search"],
        json!([{"__typename": "User", "name": "Ann"}, {"__typename": "Post", "title": "Hello"}, {"__typename": "User", "name": "Bob"}])
    );
}

#[test]
fn nested_lists_and_typename() {
    let v = single(App::default(), "{ ints __typename }");
    assert_eq!(
        v["data"],
        json!({"ints": [[1, null], []], "__typename": "Query"})
    );
}

#[test]
fn variables_and_skip() {
    let (p, _) = run(
        App::default(),
        "query Q($id: ID!, $skip: Boolean!) { user(id: $id) { id name @skip(if: $skip) } }",
        json!({"id": "2", "skip": true}),
        ExecuteOptions::default(),
    );
    assert_eq!(p[0]["data"], json!({"user": {"id": "2"}}));
}

#[test]
fn introspection() {
    let v = single(
        App::default(),
        "{ __type(name: \"User\") { name kind } __schema { queryType { name } } }",
    );
    assert_eq!(
        v["data"]["__type"],
        json!({"name": "User", "kind": "OBJECT"})
    );
    assert_eq!(v["data"]["__schema"]["queryType"]["name"], json!("Query"));
}

#[test]
fn mutation_serial() {
    let v = single(
        App::default(),
        r#"mutation { a: rename(id: "1", name: "Zed") { name } fail b: rename(id: "2", name: "Y") { name } }"#,
    );
    assert_eq!(v["data"], Value::Null);
    assert_eq!(v["errors"][0]["path"], json!(["fail"]));
    let (p, _) = run(
        App::default(),
        r#"mutation { a: rename(id: "1", name: "Zed") { name } b: rename(id: "2", name: "Y") { name } }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        p[0]["data"],
        json!({"a": {"name": "Zed"}, "b": {"name": "Y"}})
    );
}

#[test]
fn request_errors() {
    let schema = build_schema();
    assert!(schema.parse("{ nope }").is_err());
    let (p, _) = run(
        App::default(),
        "query($id: ID!) { user(id: $id) { id } }",
        json!({}),
        ExecuteOptions::default(),
    );
    assert!(p[0].get("data").is_none());
    assert!(
        p[0]["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("id")
    );
}

#[test]
fn depth_limit() {
    let schema = schema::Schema::<App>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .max_depth(2)
        .build()
        .unwrap();
    let document = schema
        .parse("{ users { posts { author { name } } } }")
        .unwrap();
    let output = block_on(schema.execute(
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    assert!(v.get("data").is_none());
    assert!(
        v["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("depth")
    );
}

#[test]
fn defer() {
    let (p, _) = run(
        App::default(),
        "{ users { id ... @defer(label: \"L\") { name posts(first: 1) { title } } } }",
        Value::Null,
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], json!({"users": [{"id": "1"}, {"id": "2"}]}));
    assert_eq!(
        p[0]["pending"],
        json!([{"id": "0", "path": ["users", 0], "label": "L"}, {"id": "1", "path": ["users", 1], "label": "L"}])
    );
    assert_eq!(p[0]["hasNext"], json!(true));
    let last = p.last().unwrap();
    assert_eq!(last["hasNext"], json!(false));
    let incremental: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(incremental.len(), 2);
    assert_eq!(
        incremental[0],
        json!({"id": "0", "data": {"name": "Ann", "posts": [{"title": "Ann post 0"}]}})
    );
    assert_eq!(
        incremental[1],
        json!({"id": "1", "data": {"name": "Bob", "posts": [{"title": "Bob post 0"}]}})
    );
    let completed: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        completed,
        json!([{"id": "0"}, {"id": "1"}])
            .as_array()
            .unwrap()
            .clone()
    );
}

#[test]
fn defer_root_fragment_with_sub_path() {
    let (p, _) = run(
        App::default(),
        "{ users { id } ... @defer { users { name } } }",
        Value::Null,
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], json!({"users": [{"id": "1"}, {"id": "2"}]}));
    assert_eq!(p[0]["pending"], json!([{"id": "0", "path": []}]));
    let incremental: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(incremental, json!([{"id": "0", "subPath": ["users", 0], "data": {"name": "Ann"}}, {"id": "0", "subPath": ["users", 1], "data": {"name": "Bob"}}]).as_array().unwrap().clone());
}

#[test]
fn stream_vec() {
    let (p, _) = run(
        App::default(),
        "{ users @stream(initialCount: 1) { id name } }",
        Value::Null,
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], json!({"users": [{"id": "1", "name": "Ann"}]}));
    assert_eq!(p[0]["pending"], json!([{"id": "0", "path": ["users"]}]));
    let incremental: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        incremental,
        json!([{"id": "0", "items": [{"id": "2", "name": "Bob"}]}])
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(p.last().unwrap()["hasNext"], json!(false));
    let completed: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(completed, json!([{"id": "0"}]).as_array().unwrap().clone());
}

#[test]
fn stream_disabled_drains() {
    let v = single(App::default(), "{ users @stream(initialCount: 1) { id } }");
    assert_eq!(v["data"], json!({"users": [{"id": "1"}, {"id": "2"}]}));
}

// A lazily streamed list from a real Stream, borrowing its parent. With
// `stall`, the list never ends, and its stream sets the flag when dropped.
#[derive(Default)]
struct StreamRoot {
    stall: Option<Arc<AtomicBool>>,
}

struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl Resolver<schema::Query::users, App> for StreamRoot {
    type Output<'obj>
        = Streamed<futures::stream::BoxStream<'obj, Result<User, Error>>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents
            .iter()
            .map(|parent| {
                let items = futures::stream::iter(users().into_iter().map(Ok));
                let Some(dropped) = &parent.stall else {
                    return Streamed::new(items.boxed());
                };
                let guard = SetOnDrop(dropped.clone());
                let stall = futures::stream::once(async move {
                    let _guard = guard;
                    futures::future::pending().await
                });
                Streamed::new(items.chain(stall).boxed())
            })
            .collect())
    }
}
impl Resolver<schema::Query::user, App> for StreamRoot {
    type Output<'obj>
        = Option<User>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj Args<schema::Query::user>,
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents.iter().map(|_| None).collect())
    }
}
impl Resolver<schema::Query::node, App> for StreamRoot {
    type Output<'obj>
        = Option<As<schema::types::User, User>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj Args<schema::Query::node>,
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents.iter().map(|_| None).collect())
    }
}
impl Resolver<schema::Query::search, App> for StreamRoot {
    type Output<'obj>
        = Vec<As<schema::types::User, User>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents.iter().map(|_| vec![]).collect())
    }
}
impl Resolver<schema::Query::ints, App> for StreamRoot {
    type Output<'obj>
        = Option<Vec<Vec<Option<i32>>>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents.iter().map(|_| None).collect())
    }
}

#[test]
fn stream_lazy() {
    let schema = schema::Schema::<App>::builder()
        .query::<StreamRoot>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap();
    let document = schema
        .parse("{ users @stream(initialCount: 0) { name posts(first: 1) { title } } }")
        .unwrap();
    let output = block_on(schema.execute(
        Roots {
            query: StreamRoot::default(),
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    ));
    let p: Vec<Value> = output
        .payloads
        .iter()
        .map(|p| serde_json::from_slice(&p.json).unwrap())
        .collect();
    assert_eq!(p[0]["data"], json!({"users": []}));
    let items: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .flat_map(|e| e["items"].as_array().cloned().unwrap())
        .collect();
    assert_eq!(items, json!([{"name": "Ann", "posts": [{"title": "Ann post 0"}]}, {"name": "Bob", "posts": [{"title": "Bob post 0"}]}]).as_array().unwrap().clone());
    assert_eq!(p.last().unwrap()["hasNext"], json!(false));
    // Disabled: the same stream drains in place.
    let output = block_on(schema.execute(
        Roots {
            query: StreamRoot::default(),
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    assert_eq!(v["data"]["users"][1]["name"], json!("Bob"));
}

#[test]
fn execution_waits_for_the_consumer_to_pull() {
    let schema = build_schema();
    let calls = Arc::new(Mutex::new(0));
    let document = schema
        .parse("{ users { id ... @defer { name } } }")
        .unwrap();
    let mut payloads = Box::pin(schema.execute_stream(
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        App {
            calls: Some(calls.clone()),
            ..Default::default()
        },
        Operation {
            document,
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
        |payload| serde_json::to_value(&payload).unwrap(),
    ));
    let first = block_on(payloads.next()).unwrap();
    assert_eq!(first["data"], json!({"users": [{"id": "1"}, {"id": "2"}]}));
    // Query.users and User.id ran; the deferred User.name waits for a pull.
    assert_eq!(*calls.lock().unwrap(), 2);
    let rest: Vec<Value> = block_on(payloads.collect());
    assert_eq!(*calls.lock().unwrap(), 3);
    assert_eq!(rest.last().unwrap()["hasNext"], json!(false));
}

#[test]
fn dropping_the_stream_cancels_execution() {
    let schema = schema::Schema::<App>::builder()
        .query::<StreamRoot>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let document = schema
        .parse("{ users @stream(initialCount: 1) { name } }")
        .unwrap();
    let mut payloads = Box::pin(schema.execute_stream(
        Roots {
            query: StreamRoot {
                stall: Some(dropped.clone()),
            },
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document,
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
        |payload| serde_json::to_value(&payload).unwrap(),
    ));
    let first = block_on(payloads.next()).unwrap();
    assert_eq!(first["data"], json!({"users": [{"name": "Ann"}]}));
    assert!(!dropped.load(Ordering::SeqCst));
    drop(payloads);
    assert!(dropped.load(Ordering::SeqCst));
}

// Objects behind `Arc` and `Box`, and owned list containers other than `Vec`.
struct OwnedShapes {
    fail_users: bool,
}

/// A collection greem knows nothing about, returned through `Items`.
struct Sequence<T>(Vec<T>);

impl<T> IntoIterator for Sequence<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'r, T> IntoIterator for &'r Sequence<T> {
    type Item = &'r T;
    type IntoIter = std::slice::Iter<'r, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

type ArcUsers = futures::stream::Iter<std::vec::IntoIter<Result<Arc<User>, Error>>>;
type SharedRows = Items<Sequence<Arc<[Option<i32>]>>>;

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl OwnedShapes {
    fn users(&self) -> Result<Streamed<ArcUsers>, Error> {
        if self.fail_users {
            return Err(Error::new("users failed"));
        }
        let items: Vec<_> = users().into_iter().map(|u| Ok(Arc::new(u))).collect();
        Ok(Streamed::new(futures::stream::iter(items)))
    }

    fn user(&self) -> Option<Box<User>> {
        users().pop().map(Box::new)
    }

    fn node(&self) -> Option<As<schema::types::User, Arc<User>>> {
        users().into_iter().next().map(|u| As::new(Arc::new(u)))
    }

    fn search(&self) -> Box<[As<schema::types::User, Box<User>>]> {
        users().into_iter().map(|u| As::new(Box::new(u))).collect()
    }

    fn ints(&self) -> Option<SharedRows> {
        Some(Items(Sequence(vec![
            Arc::from(vec![Some(1), None]),
            Arc::from(Vec::new()),
        ])))
    }
}

#[test]
fn owned_output_shapes() {
    let schema = schema::Schema::<App>::builder()
        .query::<OwnedShapes>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap();
    let run = |fail_users: bool, query: &str, options: ExecuteOptions| -> Vec<Value> {
        let document = schema.parse(query).unwrap();
        let output = block_on(schema.execute(
            Roots {
                query: OwnedShapes { fail_users },
                mutation: MutationRoot,
            },
            App::default(),
            Operation {
                document: document.clone(),
                operation_name: None,
                variables: Value::Null,
            },
            options,
        ));
        output
            .payloads
            .iter()
            .map(|p| serde_json::from_slice(&p.json).unwrap())
            .collect()
    };

    let p = run(
        false,
        r#"{ users { name } user(id: "2") { name } node(id: "1") { ... on User { name } } search { ... on User { id } } ints }"#,
        ExecuteOptions::default(),
    );
    assert_eq!(
        p[0]["data"],
        json!({
            "users": [{"name": "Ann"}, {"name": "Bob"}],
            "user": {"name": "Bob"},
            "node": {"name": "Ann"},
            "search": [{"id": "1"}, {"id": "2"}],
            "ints": [[1, null], []],
        })
    );

    let p = run(
        false,
        "{ users @stream(initialCount: 1) { name } }",
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], json!({"users": [{"name": "Ann"}]}));
    let items: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .flat_map(|e| e["items"].as_array().cloned().unwrap())
        .collect();
    assert_eq!(items, vec![json!({"name": "Bob"})]);
    assert_eq!(p.last().unwrap()["hasNext"], json!(false));

    let p = run(true, "{ users { name } }", ExecuteOptions::default());
    assert_eq!(p[0]["data"], Value::Null);
    assert_eq!(p[0]["errors"][0]["message"], json!("users failed"));
    assert_eq!(p[0]["errors"][0]["path"], json!(["users"]));
}

// List containers borrowed from the parent object.
struct BorrowedShapes {
    users: Arc<[User]>,
    ints: BoxedRows,
}

type BoxedRows = Box<[Box<[Option<i32>]>]>;

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl BorrowedShapes {
    fn users(&self) -> &Arc<[User]> {
        &self.users
    }

    fn user(&self) -> Option<&User> {
        self.users.last()
    }

    fn node(&self) -> Option<As<schema::types::User, &User>> {
        None
    }

    fn search(&self) -> Vec<As<schema::types::User, &User>> {
        Vec::new()
    }

    fn ints(&self) -> Option<&BoxedRows> {
        Some(&self.ints)
    }
}

#[test]
fn borrowed_output_shapes() {
    let schema = schema::Schema::<App>::builder()
        .query::<BorrowedShapes>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap();
    let document = schema
        .parse("{ users { name posts(first: 1) { title } } ints }")
        .unwrap();
    let output = block_on(schema.execute(
        Roots {
            query: BorrowedShapes {
                users: users().into(),
                ints: vec![vec![Some(1), None].into(), Box::default()].into(),
            },
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    assert_eq!(
        v["data"],
        json!({
            "users": [
                {"name": "Ann", "posts": [{"title": "Ann post 0"}]},
                {"name": "Bob", "posts": [{"title": "Bob post 0"}]},
            ],
            "ints": [[1, null], []],
        })
    );
}

// Wrappers borrowed from the parent object complete like their owned forms.
type UserAs = As<schema::types::User, User>;
type FallibleRows = Items<Sequence<Vec<Option<Result<i32, Error>>>>>;

struct Wrapped {
    users: Result<Vec<User>, Error>,
    user: Result<Option<User>, Error>,
    node: Option<Result<UserAs, Error>>,
    search: Vec<Either<UserAs, As<schema::types::User, Box<User>>>>,
    ints: FallibleRows,
}

fn wrapped(fail: bool) -> Wrapped {
    let error = |at: &str| Error::new(format!("{at} failed"));
    let mut search = users().into_iter();
    Wrapped {
        users: Ok(users()),
        user: if fail {
            Err(error("user"))
        } else {
            Ok(users().pop())
        },
        node: Some(if fail {
            Err(error("node"))
        } else {
            Ok(As::new(users().remove(0)))
        }),
        search: vec![
            Either::A(As::new(search.next().unwrap())),
            Either::B(As::new(Box::new(search.next().unwrap()))),
        ],
        ints: Items(Sequence(vec![
            vec![Some(Ok(1)), None, Some(Err(error("item")))],
            Vec::new(),
        ])),
    }
}

struct OwnedWrapped {
    fail: bool,
}

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl OwnedWrapped {
    fn users(&self) -> Result<Vec<User>, Error> {
        wrapped(self.fail).users
    }

    fn user(&self) -> Result<Option<User>, Error> {
        wrapped(self.fail).user
    }

    fn node(&self) -> Option<Result<UserAs, Error>> {
        wrapped(self.fail).node
    }

    fn search(&self) -> Vec<Either<UserAs, As<schema::types::User, Box<User>>>> {
        wrapped(self.fail).search
    }

    fn ints(&self) -> Option<FallibleRows> {
        Some(wrapped(self.fail).ints)
    }
}

struct BorrowedWrapped(Wrapped);

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl BorrowedWrapped {
    fn users(&self) -> &Result<Vec<User>, Error> {
        &self.0.users
    }

    fn user(&self) -> &Result<Option<User>, Error> {
        &self.0.user
    }

    fn node(&self) -> Option<&Result<UserAs, Error>> {
        self.0.node.as_ref()
    }

    fn search(&self) -> &Vec<Either<UserAs, As<schema::types::User, Box<User>>>> {
        &self.0.search
    }

    fn ints(&self) -> Option<&FallibleRows> {
        Some(&self.0.ints)
    }
}

#[test]
fn borrowed_wrappers_complete_like_owned_ones() {
    macro_rules! execute {
        ($root:ty, $value:expr) => {{
            let schema = schema::Schema::<App>::builder()
                .query::<$root>()
                .mutation::<MutationRoot>()
                .build()
                .unwrap();
            let document = schema
                .parse(
                    r#"{ users { name } user(id: "2") { name } node(id: "1") { ... on User { name } }
                         search { ... on User { name } } ints }"#,
                )
                .unwrap();
            let output = block_on(schema.execute(
                Roots {
                    query: $value,
                    mutation: MutationRoot,
                },
                App::default(),
                Operation {
                    document,
                    operation_name: None,
                    variables: Value::Null,
                },
                ExecuteOptions::default(),
            ));
            serde_json::from_slice::<Value>(&output.payloads[0].json).unwrap()
        }};
    }

    for fail in [false, true] {
        let owned = execute!(OwnedWrapped, OwnedWrapped { fail });
        let borrowed = execute!(BorrowedWrapped, BorrowedWrapped(wrapped(fail)));
        assert_eq!(owned, borrowed, "fail: {fail}");
        if !fail {
            assert_eq!(
                borrowed["data"],
                json!({
                    "users": [{"name": "Ann"}, {"name": "Bob"}],
                    "user": {"name": "Bob"},
                    "node": {"name": "Ann"},
                    "search": [{"name": "Ann"}, {"name": "Bob"}],
                    "ints": [[1, null, null], []],
                })
            );
            assert_eq!(borrowed["errors"][0]["path"], json!(["ints", 0, 2]));
        } else {
            assert_eq!(borrowed["data"]["user"], Value::Null);
            assert_eq!(borrowed["data"]["node"], Value::Null);
            assert_eq!(borrowed["errors"].as_array().unwrap().len(), 3);
        }
    }
}

/// An Int that is not `Clone`, so an owned slice of it completes only by
/// reference; it counts its drops when given a counter.
struct Count(i32, Option<Arc<AtomicUsize>>);

impl Drop for Count {
    fn drop(&mut self) {
        if let Some(dropped) = &self.1 {
            dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl ToLeaf<greem::scalars::Int> for &Count {
    fn to_leaf<'a>(self) -> Result<greem::Value<'a>, Error>
    where
        Self: 'a,
    {
        Ok(greem::Value::Int(self.0.into()))
    }
}

// Owned slices whose items are not `Clone`: leaves, objects, abstract items
// and nested lists.
struct KeptShapes;

type UserOrBoxed = Either<UserAs, As<schema::types::User, Box<User>>>;
type CountRow = Arc<[Option<Count>]>;

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl KeptShapes {
    fn users(&self) -> Arc<[User]> {
        users().into()
    }

    fn user(&self) -> Option<User> {
        None
    }

    fn node(&self) -> Option<UserAs> {
        None
    }

    fn search(&self) -> Arc<[UserOrBoxed]> {
        let mut users = users().into_iter();
        Arc::from(vec![
            Either::A(As::new(users.next().unwrap())),
            Either::B(As::new(Box::new(users.next().unwrap()))),
        ])
    }

    fn ints(&self) -> Option<Arc<[CountRow]>> {
        Some(Arc::from(vec![
            Arc::from(vec![Some(Count(1, None)), None]),
            Arc::from(Vec::new()),
        ]))
    }
}

#[test]
fn owned_slices_complete_without_cloning() {
    let schema = schema::Schema::<App>::builder()
        .query::<KeptShapes>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap();
    let run = |query: &str, options: ExecuteOptions| -> Vec<Value> {
        let output = block_on(schema.execute(
            Roots {
                query: KeptShapes,
                mutation: MutationRoot,
            },
            App::default(),
            Operation {
                document: schema.parse(query).unwrap(),
                operation_name: None,
                variables: Value::Null,
            },
            options,
        ));
        output
            .payloads
            .iter()
            .map(|p| serde_json::from_slice(&p.json).unwrap())
            .collect()
    };

    let p = run(
        "{ users { name posts(first: 1) { title } } search { ... on User { name } } ints }",
        ExecuteOptions::default(),
    );
    assert_eq!(
        p[0]["data"],
        json!({
            "users": [
                {"name": "Ann", "posts": [{"title": "Ann post 0"}]},
                {"name": "Bob", "posts": [{"title": "Bob post 0"}]},
            ],
            "search": [{"name": "Ann"}, {"name": "Bob"}],
            "ints": [[1, null], []],
        })
    );

    // A kept slice at a streamed field streams its items by reference.
    let p = run(
        "{ users @stream(initialCount: 1) { name } }",
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], json!({"users": [{"name": "Ann"}]}));
    let items: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .flat_map(|e| e["items"].as_array().cloned().unwrap())
        .collect();
    assert_eq!(items, vec![json!({"name": "Bob"})]);
}

const ROWS: i32 = 6;

type Rows = futures::stream::Iter<std::vec::IntoIter<Result<CountRow, Error>>>;

// A streamed list of owned slices, each counting its items' drops.
struct KeptRows {
    dropped: Arc<AtomicUsize>,
}

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl KeptRows {
    fn users(&self) -> Vec<User> {
        Vec::new()
    }

    fn user(&self) -> Option<User> {
        None
    }

    fn node(&self) -> Option<UserAs> {
        None
    }

    fn search(&self) -> Vec<UserAs> {
        Vec::new()
    }

    fn ints(&self) -> Option<Streamed<Rows>> {
        let rows = (0..ROWS)
            .map(|i| Ok(Arc::from(vec![Some(Count(i, Some(self.dropped.clone())))])))
            .collect::<Vec<_>>();
        Some(Streamed::new(futures::stream::iter(rows)))
    }
}

#[test]
fn kept_rows_retire_with_their_stream_turn() {
    let schema = schema::Schema::<App>::builder()
        .query::<KeptRows>()
        .mutation::<MutationRoot>()
        .stream_capacity(1)
        .build()
        .unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut payloads = Box::pin(schema.execute_stream(
        Roots {
            query: KeptRows {
                dropped: dropped.clone(),
            },
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: schema.parse("{ ints @stream }").unwrap(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
        |payload| serde_json::to_value(&payload).unwrap(),
    ));
    let mut items = Vec::new();
    let mut freed_while_streaming = 0;
    while let Some(payload) = block_on(payloads.next()) {
        for entry in payload["incremental"].as_array().into_iter().flatten() {
            items.extend(entry["items"].as_array().unwrap().iter().cloned());
        }
        if payload["hasNext"] == json!(true) {
            freed_while_streaming = dropped.load(Ordering::SeqCst);
        }
    }
    assert_eq!(items, (0..ROWS).map(|i| json!([i])).collect::<Vec<_>>());
    // Each turn holds one row and frees it when it retires, not when the
    // request ends.
    assert!(freed_while_streaming > 0);
    assert_eq!(dropped.load(Ordering::SeqCst), ROWS as usize);
}

#[test]
fn nested_defer() {
    let (p, _) = run(
        App::default(),
        "{ ... @defer { users { id ... @defer { name } } } }",
        Value::Null,
        ExecuteOptions {
            incremental: IncrementalDelivery::Enabled,
            ..Default::default()
        },
    );
    assert_eq!(p[0]["data"], json!({}));
    assert_eq!(p[0]["pending"], json!([{"id": "0", "path": []}]));
    let all: Vec<Value> = p[1..].to_vec();
    let incremental: Vec<Value> = all
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        incremental[0],
        json!({"id": "0", "data": {"users": [{"id": "1"}, {"id": "2"}]}})
    );
    let pending: Vec<Value> = all
        .iter()
        .flat_map(|p| p["pending"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        pending,
        json!([{"id": "1", "path": ["users", 0]}, {"id": "2", "path": ["users", 1]}])
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(incremental[1], json!({"id": "1", "data": {"name": "Ann"}}));
    assert_eq!(incremental[2], json!({"id": "2", "data": {"name": "Bob"}}));
    assert_eq!(p.last().unwrap()["hasNext"], json!(false));
}

#[test]
fn failed_root_is_reported_once_at_its_position() {
    let schema = schema::Schema::<App>::builder()
        .query::<Result<QueryRoot, Error>>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap();
    let document = schema.parse("{ a: users { id } b: users { id } }").unwrap();
    let output = block_on(schema.execute(
        Roots {
            query: Err(Error::new("root failed")),
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    assert_eq!(output.payloads.len(), 1);
    let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    assert_eq!(v["data"], Value::Null);
    assert_eq!(v["errors"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["errors"][0]["message"], json!("root failed"));
    assert_eq!(v["errors"][0]["path"], json!([]));
}

#[test]
fn sdl_is_the_generated_schema() {
    assert_eq!(build_schema().sdl(), schema::__private::SDL);
}

#[test]
fn a_module_generated_by_another_version_is_rejected() {
    struct Stale;
    impl greem::__private::SchemaInfo for Stale {
        const SDL: &'static str = schema::__private::SDL;
        const BUILD_VERSION: &'static str = "0.0.0-stale";
        type Query = schema::types::Query;
        type Mutation = schema::types::Mutation;
    }
    let Err(error) = greem::Schema::<Stale, App>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .build()
    else {
        panic!("a stale module must not build");
    };
    assert!(error.to_string().contains("0.0.0-stale"), "{error}");
}
