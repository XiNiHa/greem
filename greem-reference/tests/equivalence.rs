//! BFS ≡ DFS equivalence over `greem-test-app`.

use futures::executor::block_on;
use greem::{ErrorBehavior, ExecuteOptions, Operation, Roots};
use greem_test_app::app::{App, MutationRoot, QueryRoot, SubscriptionRoot, build_schema};
use serde_json::{Value, json};
use std::sync::Mutex;

fn options(behavior: ErrorBehavior) -> ExecuteOptions {
    ExecuteOptions {
        default_error_behavior: behavior,
        ..Default::default()
    }
}

/// Runs both executors and returns (bfs response, reference response, reference calls).
fn both(
    make_app: impl Fn() -> App,
    query: &str,
    variables: Value,
    behavior: ErrorBehavior,
) -> (Value, Value, u64) {
    let variables = match variables {
        Value::Null => Default::default(),
        Value::Object(map) => map,
        other => panic!("variables must be a map: {other}"),
    };
    let schema = build_schema();
    let document = schema.parse(query).unwrap_or_else(|e| panic!("{e:?}"));
    let output = block_on(
        schema.execute(
            Roots::query(QueryRoot)
                .with_mutation(MutationRoot)
                .with_subscription(SubscriptionRoot),
            make_app(),
            Operation::new(document.clone()).with_variables(variables.clone()),
            options(behavior),
        ),
    );
    let bfs: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    let reference = block_on(greem_reference::execute(
        &schema,
        Roots::query(QueryRoot)
            .with_mutation(MutationRoot)
            .with_subscription(SubscriptionRoot),
        make_app(),
        Operation::new(document.clone()).with_variables(variables),
        options(behavior),
    ));
    (bfs, reference.response().clone(), reference.calls)
}

fn sorted_errors(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = v
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .map(|e| serde_json::to_string(e).unwrap())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn assert_equivalent(bfs: &Value, reference: &Value) {
    assert_eq!(
        bfs.get("data"),
        reference.get("data"),
        "data differs\nbfs: {bfs}\nref: {reference}"
    );
    assert_eq!(
        sorted_errors(bfs),
        sorted_errors(reference),
        "errors differ\nbfs: {bfs}\nref: {reference}"
    );
}

fn check(
    make_app: impl Fn() -> App,
    query: &str,
    variables: Value,
    behavior: ErrorBehavior,
) -> (Value, Value) {
    let (bfs, reference, _) = both(make_app, query, variables, behavior);
    assert_equivalent(&bfs, &reference);
    (bfs, reference)
}

#[test]
fn flat_and_nested() {
    let (bfs, _) = check(
        App::default,
        "{ users { id name email posts(first: 1) { title author { name } } } }",
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(
        bfs["data"]["users"][1]["posts"][0]["author"]["name"],
        json!("Bob")
    );
}

#[test]
fn interfaces_and_unions() {
    check(
        App::default,
        r#"{ a: node(id: "1") { id ... on User { name } ... on Post { title } } b: node(id: "p1") { __typename id ... on Post { title } } search { __typename ... on User { name } ... on Post { title } } }"#,
        Value::Null,
        ErrorBehavior::Propagate,
    );
}

#[test]
fn nested_lists_and_typename() {
    let (bfs, _) = check(
        App::default,
        "{ ints __typename }",
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(
        bfs["data"],
        json!({"ints": [[1, null], []], "__typename": "Query"})
    );
}

#[test]
fn nullable_propagation() {
    let (bfs, _) = check(
        || App {
            fail_name: true,
            ..Default::default()
        },
        "{ users { name } user(id: \"2\") { name } }",
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(bfs["data"], Value::Null);
    assert_eq!(bfs["errors"].as_array().unwrap().len(), 2);
}

#[test]
fn nullable_absorbs() {
    let (bfs, _) = check(
        || App {
            fail_email: true,
            ..Default::default()
        },
        "{ users { name email } }",
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(bfs["data"]["users"][0]["email"], Value::Null);
}

#[test]
fn error_behavior_null() {
    let (bfs, _) = check(
        || App {
            fail_name: true,
            ..Default::default()
        },
        "{ users { name email } }",
        Value::Null,
        ErrorBehavior::Null,
    );
    assert_eq!(bfs["data"]["users"][1]["name"], Value::Null);
    assert_eq!(bfs["data"]["users"][0]["name"], json!("Ann"));
}

#[test]
fn error_behavior_halt() {
    let (bfs, reference) = check(
        || App {
            fail_name: true,
            ..Default::default()
        },
        "{ users { name email } }",
        Value::Null,
        ErrorBehavior::Halt,
    );
    assert_eq!(bfs["data"], Value::Null);
    assert_eq!(bfs["errors"].as_array().unwrap().len(), 1);
    assert_eq!(reference["errors"].as_array().unwrap().len(), 1);
}

#[test]
fn mutation_serial() {
    let (bfs, _) = check(
        App::default,
        r#"mutation { a: rename(id: "1", name: "Zed") { name } fail b: rename(id: "2", name: "Y") { name } }"#,
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(bfs["data"], Value::Null);
    assert_eq!(bfs["errors"][0]["path"], json!(["fail"]));
    let (bfs, _) = check(
        App::default,
        r#"mutation { a: rename(id: "1", name: "Zed") { name } b: rename(id: "2", name: "Y") { name } }"#,
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(
        bfs["data"],
        json!({"a": {"name": "Zed"}, "b": {"name": "Y"}})
    );
}

#[test]
fn introspection() {
    let (bfs, _) = check(
        App::default,
        "{ __type(name: \"User\") { name kind } }",
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(
        bfs["data"]["__type"],
        json!({"name": "User", "kind": "OBJECT"})
    );
}

#[test]
fn stream_disabled() {
    let (bfs, _) = check(
        App::default,
        "{ users @stream(initialCount: 1) { id } }",
        Value::Null,
        ErrorBehavior::Propagate,
    );
    assert_eq!(bfs["data"], json!({"users": [{"id": "1"}, {"id": "2"}]}));
}

#[test]
fn request_error() {
    let (bfs, reference) = check(
        App::default,
        "query($id: ID!) { user(id: $id) { id } }",
        json!({}),
        ErrorBehavior::Propagate,
    );
    assert!(bfs.get("data").is_none());
    assert!(reference.get("data").is_none());
}

#[test]
fn reference_calls_once_per_object() {
    // BFS: one call per field per scope; the reference: one call per object.
    let schema = build_schema();
    let query = "{ users { posts { title author { name } } } }";
    let document = schema.parse(query).unwrap();
    let bfs_calls = std::sync::Arc::new(Mutex::new(0usize));
    block_on(
        schema.execute(
            Roots::query(QueryRoot)
                .with_mutation(MutationRoot)
                .with_subscription(SubscriptionRoot),
            App {
                calls: Some(bfs_calls.clone()),
                ..Default::default()
            },
            Operation::new(document.clone()),
            ExecuteOptions::default(),
        ),
    );
    let reference = block_on(greem_reference::execute(
        &schema,
        Roots::query(QueryRoot)
            .with_mutation(MutationRoot)
            .with_subscription(SubscriptionRoot),
        App::default(),
        Operation::new(document.clone()),
        ExecuteOptions::default(),
    ));
    // BFS: users(1) + posts(1) + title(1) + author(1) + name(1) = 5 scope calls.
    // Reference: users(1) + posts(2) + title(4) + author(4) + name(4) = 15.
    let bfs_calls = *bfs_calls.lock().unwrap();
    assert_eq!(bfs_calls, 5);
    assert_eq!(reference.calls, 15);
    assert!(reference.calls > bfs_calls as u64);
}

/// A subscription root that failed as a whole (`Result<S, Error>` as the
/// root value) is `data: null` with the error at the root in both executors.
#[test]
fn a_failed_subscription_root_is_data_null_in_both() {
    let schema = greem_test_app::schema::Schema::<App>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .subscription::<Result<SubscriptionRoot, greem::Error>>()
        .build()
        .unwrap();
    let document = schema.parse("subscription { numbers(count: 2) }").unwrap();
    let roots = || {
        Roots::query(QueryRoot)
            .with_mutation(MutationRoot)
            .with_subscription(Err::<SubscriptionRoot, _>(greem::Error::new("no root")))
    };
    let output = block_on(schema.execute(
        roots(),
        App::default(),
        Operation::new(document.clone()),
        ExecuteOptions::default(),
    ));
    let bfs: Vec<Value> = output
        .payloads
        .iter()
        .map(|p| serde_json::from_slice(&p.json).unwrap())
        .collect();
    let reference = block_on(greem_reference::execute(
        &schema,
        roots(),
        App::default(),
        Operation::new(document),
        ExecuteOptions::default(),
    ));
    let expected = vec![json!({"data": null, "errors": [{"message": "no root", "path": []}]})];
    assert_eq!(bfs, expected);
    assert_eq!(reference.responses, expected);
}
