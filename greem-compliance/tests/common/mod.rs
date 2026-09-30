#![allow(dead_code)]

use futures::executor::block_on;
use greem::{ExecuteOptions, IncrementalDelivery, Operation, Roots};
use greem_compliance::world::{MutationRoot, QueryRoot, World, build_schema};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type Calls = Vec<(&'static str, usize)>;

/// Runs the BFS executor; returns every payload as JSON and the resolver call log.
pub fn run(
    world: World,
    query: &str,
    variables: Value,
    options: ExecuteOptions,
) -> (Vec<Value>, Calls) {
    run_at_capacity(world, query, variables, options, None)
}

/// [`run`] with the items buffered per stream turn set (`None`: the default).
pub fn run_at_capacity(
    world: World,
    query: &str,
    variables: Value,
    options: ExecuteOptions,
    stream_capacity: Option<usize>,
) -> (Vec<Value>, Calls) {
    let schema = match stream_capacity {
        Some(capacity) => greem_compliance::schema::Schema::<World>::builder()
            .query::<QueryRoot>()
            .mutation::<MutationRoot>()
            .stream_capacity(capacity)
            .build()
            .expect("property schema builds"),
        None => build_schema(),
    };
    let document = match schema.parse(query) {
        Ok(d) => d,
        Err(e) => {
            return (
                vec![serde_json::from_slice(&e.into_payload().json).unwrap()],
                Vec::new(),
            );
        }
    };
    let calls = world.calls.clone();
    let output = block_on(schema.execute(
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        world,
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
    let calls = calls.lock().unwrap().clone();
    (payloads, calls)
}

/// Drives the execution with a no-op waker until it stalls or finishes, and
/// returns the payloads shipped so far: for queries that never complete.
pub fn run_until_stalled(world: World, query: &str, options: ExecuteOptions) -> Vec<Value> {
    use std::task::{Context, Poll};
    let schema = build_schema();
    let document = schema.parse(query).unwrap();
    let mut payloads = Vec::new();
    {
        let future = schema.execute_with(
            Roots {
                query: QueryRoot,
                mutation: MutationRoot,
            },
            world,
            Operation {
                document: &document,
                operation_name: None,
                variables: Value::Null,
            },
            options,
            |payload| payloads.push(serde_json::to_value(&payload).unwrap()),
        );
        let mut future = std::pin::pin!(future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        for _ in 0..1000 {
            if let Poll::Ready(()) = future.as_mut().poll(&mut cx) {
                break;
            }
        }
    }
    payloads
}

/// The BFS single (non-incremental) response.
pub fn single(
    world: World,
    query: &str,
    variables: Value,
    options: ExecuteOptions,
) -> (Value, Calls) {
    let (mut payloads, calls) = run(
        world,
        query,
        variables,
        ExecuteOptions {
            incremental: IncrementalDelivery::Disabled,
            ..options
        },
    );
    (payloads.remove(0), calls)
}

/// The reference executor's response and call count.
pub fn reference(
    world: World,
    query: &str,
    variables: Value,
    options: ExecuteOptions,
) -> (Value, u64) {
    let schema = build_schema();
    let document = match schema.parse(query) {
        Ok(d) => d,
        Err(e) => return (serde_json::from_slice(&e.into_payload().json).unwrap(), 0),
    };
    let out = block_on(greem_reference::execute(
        &schema,
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        world,
        Operation {
            document: &document,
            operation_name: None,
            variables,
        },
        options,
    ));
    (out.response, out.calls)
}

pub fn sorted_errors(v: &Value) -> Vec<Value> {
    let mut errors = v
        .get("errors")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    errors.sort_by_key(|e| e.to_string());
    errors
}

pub fn assert_equivalent(bfs: &Value, reference: &Value) {
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

/// Folds an incremental payload stream into one response: each `data` entry
/// is merged at `path` + `subPath`, each `items` entry appended at `path`.
pub fn fold(payloads: &[Value]) -> Value {
    let mut data = payloads[0].get("data").cloned().unwrap_or(Value::Null);
    let mut errors: Vec<Value> = payloads[0]
        .get("errors")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    let mut paths: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for payload in payloads {
        for pending in payload
            .get("pending")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
        {
            paths.insert(
                pending["id"].as_str().unwrap().to_owned(),
                pending["path"].as_array().cloned().unwrap(),
            );
        }
        for entry in payload
            .get("incremental")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
        {
            let mut path = paths[entry["id"].as_str().unwrap()].clone();
            if let Some(sub) = entry.get("subPath").and_then(|s| s.as_array()) {
                path.extend(sub.iter().cloned());
            }
            if let Some(object) = entry.get("data") {
                let target = navigate(&mut data, &path);
                if let (Value::Object(target), Value::Object(object)) = (target, object) {
                    for (k, v) in object {
                        target.insert(k.clone(), v.clone());
                    }
                }
            }
            if let Some(items) = entry.get("items").and_then(|i| i.as_array()) {
                let target = navigate(&mut data, &path);
                if let Value::Array(list) = target {
                    list.extend(items.iter().cloned());
                }
            }
            errors.extend(
                entry
                    .get("errors")
                    .and_then(|e| e.as_array())
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    let mut out = Map::new();
    out.insert("data".into(), data);
    if !errors.is_empty() {
        out.insert("errors".into(), Value::Array(errors));
    }
    Value::Object(out)
}

fn navigate<'v>(value: &'v mut Value, path: &[Value]) -> &'v mut Value {
    let mut cursor = value;
    for segment in path {
        cursor = match segment {
            Value::String(key) => cursor
                .as_object_mut()
                .expect("object on path")
                .get_mut(key)
                .expect("key on path"),
            Value::Number(index) => cursor
                .as_array_mut()
                .expect("list on path")
                .get_mut(index.as_u64().unwrap() as usize)
                .expect("index on path"),
            _ => unreachable!(),
        };
    }
    cursor
}

pub fn has_failed_group(payloads: &[Value]) -> bool {
    payloads.iter().any(|p| {
        p.get("completed")
            .and_then(|c| c.as_array())
            .is_some_and(|entries| entries.iter().any(|e| e.get("errors").is_some()))
    })
}

/// Errors as a sorted set of (path, message).
pub fn error_keys(v: &Value) -> Vec<(String, String)> {
    let mut keys: Vec<(String, String)> = v
        .get("errors")
        .and_then(|e| e.as_array())
        .map(|errors| {
            errors
                .iter()
                .map(|e| (e["path"].to_string(), e["message"].to_string()))
                .collect()
        })
        .unwrap_or_default();
    keys.sort();
    keys.dedup();
    keys
}

/// True when some error sits beneath a position that propagation nulled: the
/// work under that null is dropped by incremental delivery, so the fold
/// property does not apply (ticket 10's precondition).
pub fn errors_under_null(response: &Value) -> bool {
    let Some(errors) = response.get("errors").and_then(|e| e.as_array()) else {
        return false;
    };
    let data = response.get("data").unwrap_or(&Value::Null);
    errors.iter().any(|error| {
        let Some(path) = error.get("path").and_then(|p| p.as_array()) else {
            return false;
        };
        let mut cursor = data;
        for segment in &path[..path.len().saturating_sub(1)] {
            cursor = match segment {
                Value::String(key) => match cursor.get(key) {
                    Some(v) => v,
                    None => return true,
                },
                Value::Number(i) => match cursor.get(i.as_u64().unwrap() as usize) {
                    Some(v) => v,
                    None => return true,
                },
                _ => return true,
            };
            if cursor.is_null() {
                return true;
            }
        }
        cursor.is_null()
    })
}
