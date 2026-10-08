//! Depth and lifecycle evidence (ticket 17): completion, cancellation and
//! panics at the limit and at twice the default on a 2 MiB debug stack; long
//! streams and many mutation roots add width, not depth.

mod common;

use futures::StreamExt;
use futures::executor::block_on;
use greem::{ExecuteOptions, IncrementalDelivery, Operation, Roots};
use greem_compliance::world::{MutationRoot, QueryRoot, World, take_drops};
use serde_json::Value;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::Context;

const STACK: usize = 2 << 20;

fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn deep_query(depth: u32) -> String {
    let mut query = String::from("name");
    for _ in 0..depth {
        query = format!("friends {{ {query} }}");
    }
    format!("{{ users(first: 1) {{ {query} }} }}")
}

fn schema_with_depth(limit: u32) -> greem_compliance::world::Schema {
    greem_compliance::schema::Schema::<World>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .max_depth(limit)
        .build()
        .unwrap()
}

/// Responses nested past serde_json's default 128 levels need the unbounded parser.
fn parse_deep(bytes: &[u8]) -> Value {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    de.disable_recursion_limit();
    serde::Deserialize::deserialize(&mut de).unwrap()
}

fn run_deep(depth: u32, limit: u32, world: World) -> Value {
    let schema = schema_with_depth(limit);
    let document = schema.parse(&deep_query(depth)).unwrap();
    let output = block_on(schema.execute(
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        },
        world,
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    parse_deep(&output.payloads[0].json)
}

fn deepest_name(mut v: &Value, depth: u32) -> &Value {
    v = &v["data"]["users"][0];
    for _ in 0..depth {
        // Two users: user0's only friend is user1 and vice versa; the other slot is null.
        let friends = v["friends"].as_array().unwrap();
        v = friends.iter().find(|f| !f.is_null()).unwrap();
    }
    &v["name"]
}

/// Twice the default `max_depth`; the documented evidence bar.
const TWICE_DEFAULT: u32 = 64;

#[test]
fn completion_at_the_limit_and_at_twice_the_default_on_a_2mib_stack() {
    on_small_stack(|| {
        // users adds one level; `friends` nested `d` times adds d.
        let v = run_deep(31, 32, World::seeded(2, 0));
        assert_eq!(deepest_name(&v, 31), "user1");
        let v = run_deep(32, 32, World::seeded(2, 0));
        assert!(
            v.get("data").is_none(),
            "one past the limit is rejected: {v}"
        );
        let v = run_deep(TWICE_DEFAULT - 1, TWICE_DEFAULT, World::seeded(2, 0));
        assert_eq!(deepest_name(&v, TWICE_DEFAULT - 1), "user1");
    });
}

/// Drops the execution future after `polls` polls and returns the drop log.
fn cancel_after(depth: u32, world: World, polls: usize) -> Vec<(&'static str, u32)> {
    let schema = schema_with_depth(64);
    let document = schema.parse(&deep_query(depth)).unwrap();
    take_drops();
    {
        let future = schema.execute(
            Roots {
                query: QueryRoot,
                mutation: MutationRoot,
            },
            world,
            Operation {
                document: document.clone(),
                operation_name: None,
                variables: Value::Null,
            },
            ExecuteOptions::default(),
        );
        let mut future = pin!(future);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        for _ in 0..polls {
            if future.as_mut().poll(&mut cx).is_ready() {
                panic!("execution finished before it could be cancelled");
            }
        }
        // Dropping here cancels mid-execution.
    }
    take_drops()
}

fn assert_children_before_parents(drops: &[(&'static str, u32)]) {
    // `depth` on a User counts nesting; a deeper object must drop before any shallower one.
    let users: Vec<u32> = drops
        .iter()
        .filter(|(t, _)| *t == "User")
        .map(|(_, d)| *d)
        .collect();
    assert!(!users.is_empty());
    for window in users.windows(2) {
        assert!(
            window[0] >= window[1],
            "drop order not child-before-parent: {drops:?}"
        );
    }
}

#[test]
fn cancellation_at_the_deepest_generation_drops_children_first() {
    on_small_stack(|| {
        let world = World {
            gate_field: Some("User.name"),
            track_drops: true,
            ..World::seeded(2, 0)
        };
        let drops = cancel_after(TWICE_DEFAULT - 1, world, 200);
        // The deepest generation was reached: a user at depth 63 was created and dropped.
        assert!(
            drops.iter().any(|&(_, d)| d == TWICE_DEFAULT - 1),
            "{drops:?}"
        );
        assert_children_before_parents(&drops);
    });
}

#[test]
fn panic_in_the_deepest_resolver_unwinds_with_children_first() {
    on_small_stack(|| {
        let world = World {
            panic_field: Some("User.name"),
            track_drops: true,
            ..World::seeded(2, 0)
        };
        take_drops();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_deep(TWICE_DEFAULT - 1, TWICE_DEFAULT, world)
        }));
        assert!(result.is_err());
        let drops = take_drops();
        assert!(
            drops.iter().any(|&(_, d)| d == TWICE_DEFAULT - 1),
            "{drops:?}"
        );
        assert_children_before_parents(&drops);
    });
}

#[test]
fn a_thousand_stream_turns_do_not_add_depth() {
    on_small_stack(|| {
        let schema = greem_compliance::schema::Schema::<World>::builder()
            .query::<QueryRoot>()
            .mutation::<MutationRoot>()
            .max_depth(64)
            .stream_capacity(1)
            .build()
            .unwrap();
        // A deferred fragment per item: groups must be reclaimed like turns.
        let query = "{ users(first: 1) { drafts @stream(initialCount: 0) { id owner { name } ... @defer { title } } } }";
        greem::__private::MAX_LIVE_GROUPS.store(0, std::sync::atomic::Ordering::Relaxed);
        greem::__private::MAX_LIVE_TURNS.store(0, std::sync::atomic::Ordering::Relaxed);
        let document = schema.parse(query).unwrap();
        let mut entries = 0usize;
        let mut items = 0usize;
        block_on(
            schema
                .execute_stream(
                    Roots {
                        query: QueryRoot,
                        mutation: MutationRoot,
                    },
                    World::seeded(1, 1000),
                    Operation {
                        document: document.clone(),
                        operation_name: None,
                        variables: Value::Null,
                    },
                    ExecuteOptions {
                        incremental: IncrementalDelivery::Enabled,
                        ..Default::default()
                    },
                    |payload| {
                        let json: Value = serde_json::to_value(&payload).unwrap();
                        for entry in json
                            .get("incremental")
                            .and_then(|i| i.as_array())
                            .into_iter()
                            .flatten()
                        {
                            // Deferred data entries ride alongside; only item entries are turns.
                            if let Some(list) = entry["items"].as_array() {
                                entries += 1;
                                items += list.len();
                            }
                        }
                    },
                )
                .for_each(|()| async {}),
        );
        assert_eq!(items, 1000);
        assert_eq!(entries, 1000, "one turn per item at capacity 1");
        let groups = greem::__private::MAX_LIVE_GROUPS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            groups <= 32,
            "completed groups are reclaimed: {groups} groups were live at once"
        );
        let turns = greem::__private::MAX_LIVE_TURNS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            turns <= 8,
            "retired turn slots are reused: {turns} turns were live at once"
        );
    });
}

#[test]
fn a_thousand_streamed_errors_are_retired_with_their_turns() {
    on_small_stack(|| {
        let schema = greem_compliance::schema::Schema::<World>::builder()
            .query::<QueryRoot>()
            .mutation::<MutationRoot>()
            .stream_capacity(1)
            .build()
            .unwrap();
        let query = "{ noisy(count: 1000) @stream(initialCount: 0) }";
        let document = schema.parse(query).unwrap();
        greem::__private::MAX_LIVE_TURNS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut errors = 0usize;
        let mut nulls = 0usize;
        block_on(
            schema
                .execute_stream(
                    Roots {
                        query: QueryRoot,
                        mutation: MutationRoot,
                    },
                    World::seeded(1, 0),
                    Operation {
                        document: document.clone(),
                        operation_name: None,
                        variables: Value::Null,
                    },
                    ExecuteOptions {
                        incremental: IncrementalDelivery::Enabled,
                        ..Default::default()
                    },
                    |payload| {
                        let json: Value = serde_json::to_value(&payload).unwrap();
                        for entry in json
                            .get("incremental")
                            .and_then(|i| i.as_array())
                            .into_iter()
                            .flatten()
                        {
                            nulls += entry["items"]
                                .as_array()
                                .map_or(0, |i| i.iter().filter(|v| v.is_null()).count());
                            errors += entry["errors"].as_array().map_or(0, |e| e.len());
                        }
                    },
                )
                .for_each(|()| async {}),
        );
        assert_eq!((nulls, errors), (1000, 1000));
        let turns = greem::__private::MAX_LIVE_TURNS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            turns <= 8,
            "error records retire with their turns: {turns} turns were live at once"
        );
    });
}

#[test]
fn abandoned_nested_groups_are_reclaimed_after_parent_failure() {
    // Every item's deferred `title` fails (non-null), so the nested `id`
    // fragment is abandoned; those groups must still be reclaimed.
    on_small_stack(|| {
        let schema = greem_compliance::schema::Schema::<World>::builder()
            .query::<QueryRoot>()
            .mutation::<MutationRoot>()
            .stream_capacity(1)
            .build()
            .unwrap();
        let query = "{ users(first: 1) { drafts @stream(initialCount: 0) { ... @defer { title ... @defer { id } } } } }";
        let document = schema.parse(query).unwrap();
        let failures = (0..200)
            .map(|i| greem_compliance::world::Failure {
                type_name: "Post",
                field: "title",
                object: i,
            })
            .collect();
        let world = World {
            failures,
            ..World::seeded(1, 200)
        };
        greem::__private::MAX_LIVE_GROUPS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut failed = 0usize;
        block_on(
            schema
                .execute_stream(
                    Roots {
                        query: QueryRoot,
                        mutation: MutationRoot,
                    },
                    world,
                    Operation {
                        document: document.clone(),
                        operation_name: None,
                        variables: Value::Null,
                    },
                    ExecuteOptions {
                        incremental: IncrementalDelivery::Enabled,
                        ..Default::default()
                    },
                    |payload| {
                        let json: Value = serde_json::to_value(&payload).unwrap();
                        failed += json
                            .get("completed")
                            .and_then(|c| c.as_array())
                            .map_or(0, |c| {
                                c.iter().filter(|e| e.get("errors").is_some()).count()
                            });
                    },
                )
                .for_each(|()| async {}),
        );
        assert_eq!(failed, 200);
        let groups = greem::__private::MAX_LIVE_GROUPS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            groups <= 32,
            "abandoned nested groups are reclaimed: {groups} groups were live at once"
        );
    });
}

#[test]
fn a_thousand_mutation_roots_do_not_add_depth() {
    on_small_stack(|| {
        let schema = schema_with_depth(64);
        let mut nested = String::from("name");
        for _ in 0..30 {
            nested = format!("friends {{ {nested} }}");
        }
        let roots: Vec<String> = (0..1000)
            .map(|i| format!("m{i}: rename(id: \"0\", name: \"x\") {{ {nested} }}"))
            .collect();
        let query = format!("mutation {{ {} }}", roots.join(" "));
        let document = schema.parse(&query).unwrap();
        let output = block_on(schema.execute(
            Roots {
                query: QueryRoot,
                mutation: MutationRoot,
            },
            World::seeded(2, 0),
            Operation {
                document: document.clone(),
                operation_name: None,
                variables: Value::Null,
            },
            ExecuteOptions::default(),
        ));
        let v = parse_deep(&output.payloads[0].json);
        assert!(v.get("errors").is_none());
        assert_eq!(v["data"].as_object().unwrap().len(), 1000);
        let mut cursor = &v["data"]["m999"];
        for _ in 0..30 {
            cursor = cursor["friends"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| !f.is_null())
                .unwrap();
        }
        assert_eq!(cursor["name"], "user0");
    });
}

#[test]
fn reference_executor_runs_on_a_larger_stack() {
    // The reference executor recurses several Rust frames per level; the harness
    // gives it room instead of shaping the limit around an oracle.
    let handle = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let schema = schema_with_depth(64);
            let document = schema.parse(&deep_query(40)).unwrap();
            block_on(greem_reference::execute(
                &schema,
                Roots {
                    query: QueryRoot,
                    mutation: MutationRoot,
                },
                World::seeded(2, 0),
                Operation {
                    document: document.clone(),
                    operation_name: None,
                    variables: Value::Null,
                },
                ExecuteOptions::default(),
            ))
            .response
        })
        .unwrap();
    let v = handle.join().unwrap();
    assert_eq!(deepest_name(&v, 40), "user0");
    let _ = Arc::new(());
}
