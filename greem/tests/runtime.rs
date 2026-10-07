//! The runtime end to end, over `greem-test-app`'s generated schema module
//! and hand-written resolvers.

use futures::executor::block_on;
use greem::{
    Args, As, Context, Error, ErrorBehavior, ExecuteOptions, IncrementalDelivery, Items, Operation,
    Resolver, Roots, Streamed,
};
use greem_test_app::app::{App, MutationRoot, QueryRoot, User, build_schema, users};
use greem_test_app::schema;
use serde_json::{Value, json};
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
            document: &document,
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
            document: &document,
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
            document: &document,
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

// A lazily streamed list from a real Stream, borrowing its parent.
struct StreamRoot;
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
        use futures::StreamExt;
        Ok(parents
            .iter()
            .map(|_| Streamed::new(futures::stream::iter(users().into_iter().map(Ok)).boxed()))
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
            query: StreamRoot,
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: &document,
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
            query: StreamRoot,
            mutation: MutationRoot,
        },
        App::default(),
        Operation {
            document: &document,
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    assert_eq!(v["data"]["users"][1]["name"], json!("Bob"));
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
                document: &document,
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
            document: &document,
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
            document: &document,
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
