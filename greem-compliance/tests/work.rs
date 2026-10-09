//! Barrier and poll work follows what changed, not the size of the request
//! (issue 23): a `@stream` under a list ships a bounded number of parents
//! per barrier, so the work per barrier must not grow with the list.

use futures::StreamExt;
use futures::executor::block_on;
use greem::{ExecuteOptions, IncrementalDelivery, Operation, Roots};
use greem_compliance::world::{MutationRoot, QueryRoot, World};
use serde_json::Value;
use std::sync::atomic::Ordering::Relaxed;

struct Work {
    payloads: f64,
    scopes_polled: f64,
    groups_examined: f64,
    objects_announced: f64,
    parents_visited: f64,
}

/// Runs `query` with `{users}` substituted and returns the work per
/// payload. The counters are process-wide, so this file holds one test.
fn run(query: &str, users: u32, items_per_user: u32) -> Work {
    let schema = greem_compliance::world::build_schema();
    let query = query.replace("{users}", &users.to_string());
    let document = schema.parse(&query).unwrap();
    let before = (
        greem::__private::SCOPES_POLLED.load(Relaxed),
        greem::__private::GROUPS_EXAMINED.load(Relaxed),
        greem::__private::OBJECTS_ANNOUNCED.load(Relaxed),
        greem::__private::STREAM_PARENTS_VISITED.load(Relaxed),
    );
    let mut payloads = 0usize;
    let mut items = 0usize;
    block_on(
        schema
            .execute_stream(
                Roots {
                    query: QueryRoot,
                    mutation: MutationRoot,
                },
                World::seeded(users, 3),
                Operation {
                    document,
                    operation_name: None,
                    variables: Value::Null,
                },
                ExecuteOptions {
                    incremental: IncrementalDelivery::Enabled,
                    ..Default::default()
                },
                |payload| {
                    let json: Value = serde_json::to_value(&payload).unwrap();
                    payloads += 1;
                    for entry in json["incremental"].as_array().into_iter().flatten() {
                        items += entry["items"].as_array().map_or(0, Vec::len);
                    }
                },
            )
            .for_each(|()| async {}),
    );
    assert_eq!(
        items as u32,
        users * items_per_user,
        "every streamed item ships"
    );
    let per = |now: usize, before: usize| (now - before) as f64 / payloads as f64;
    Work {
        payloads: payloads as f64,
        scopes_polled: per(greem::__private::SCOPES_POLLED.load(Relaxed), before.0),
        groups_examined: per(greem::__private::GROUPS_EXAMINED.load(Relaxed), before.1),
        objects_announced: per(greem::__private::OBJECTS_ANNOUNCED.load(Relaxed), before.2),
        parents_visited: per(
            greem::__private::STREAM_PARENTS_VISITED.load(Relaxed),
            before.3,
        ),
    }
}

/// The work per payload at 1,600 users is no more than at 400.
fn assert_bounded(query: &str, items_per_user: u32) {
    let small = run(query, 400, items_per_user);
    let large = run(query, 1600, items_per_user);
    assert!(
        large.payloads > 3.0 * small.payloads,
        "the larger list takes more barriers: {} vs {}",
        large.payloads,
        small.payloads
    );
    for (what, s, l) in [
        ("scopes polled", small.scopes_polled, large.scopes_polled),
        (
            "groups examined",
            small.groups_examined,
            large.groups_examined,
        ),
        (
            "objects announced",
            small.objects_announced,
            large.objects_announced,
        ),
        (
            "stream parents visited",
            small.parents_visited,
            large.parents_visited,
        ),
    ] {
        assert!(
            l <= s * 1.5 + 1.0,
            "{what} per payload grew with the list ({query}): {s:.1} at 400 users, {l:.1} at 1600"
        );
    }
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the work counters count in debug builds only"
)]
fn work_per_barrier_does_not_grow_with_the_list() {
    assert_bounded(
        "{ users(first: {users}) { id drafts @stream(initialCount: 1) { id title } } }",
        2,
    );
    // A scope whose stream ends early beside one that keeps going: once
    // quiescent it must cost nothing more.
    assert_bounded(
        "{ a: users(first: {users}) { id drafts @stream(initialCount: 10) { id } } \
           b: users(first: {users}) { id posts @stream(initialCount: 1) { id } } }",
        2,
    );
}
