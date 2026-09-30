//! Property tests: BFS ≡ DFS over generated documents, worlds and
//! interleavings; incremental fold; breadth-first call counts; determinism.

mod common;

use common::*;
use greem::{ErrorBehavior, ExecuteOptions, IncrementalDelivery};
use greem_compliance::world::{Failure, World};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

mod generator;
use generator::*;

fn failure() -> impl Strategy<Value = Failure> {
    (
        prop::sample::select(vec![
            ("User", "id"),
            ("User", "name"),
            ("User", "email"),
            ("User", "posts"),
            ("User", "drafts"),
            ("Post", "title"),
            ("Post", "author"),
        ]),
        0..3u32,
    )
        .prop_map(|((type_name, field), object)| Failure {
            type_name,
            field,
            object: if type_name == "Post" {
                object * 100 + object % 2
            } else {
                object
            },
        })
}

fn world() -> impl Strategy<Value = (u32, u32, BTreeSet<Failure>)> {
    (
        0..4u32,
        0..3u32,
        prop::collection::btree_set(failure(), 0..3),
    )
}

fn interleaving() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(0..3u32, 0..6)
}

/// Items buffered per stream turn: small values split a stream into many
/// turns whose subtrees finish at different barriers; `None` is the default.
fn stream_capacity() -> impl Strategy<Value = Option<usize>> {
    prop_oneof![
        3 => Just(Some(1)),
        2 => Just(Some(2)),
        1 => Just(Some(3)),
        2 => Just(None),
    ]
}

fn make_world(users: u32, posts: u32, failures: &BTreeSet<Failure>, yields: &[u32]) -> World {
    World {
        users,
        posts_per_user: posts,
        failures: failures.clone(),
        yields: yields.to_vec(),
        ..World::default()
    }
}

fn variables(flag: bool) -> Value {
    json!({"flag": flag, "patch": {"name": "v", "email": null}})
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn bfs_equals_dfs(doc in document(false), (users, posts, failures) in world(), yields in interleaving(), flag in any::<bool>(), behavior in prop::sample::select(vec![ErrorBehavior::Propagate, ErrorBehavior::Null])) {
        let options = ExecuteOptions { error_behavior: behavior, ..Default::default() };
        let (bfs, _) = single(make_world(users, posts, &failures, &yields), &doc, variables(flag), options);
        let (reference, _) = reference(make_world(users, posts, &failures, &yields), &doc, variables(flag), options);
        prop_assert_eq!(bfs.get("data"), reference.get("data"), "doc: {}\nbfs: {}\nref: {}", doc, bfs, reference);
        prop_assert_eq!(sorted_errors(&bfs), sorted_errors(&reference), "doc: {}\nbfs: {}\nref: {}", doc, bfs, reference);
    }

    #[test]
    fn halt_is_one_error_from_null_mode(doc in document(false), (users, posts, failures) in world(), flag in any::<bool>()) {
        let halt = ExecuteOptions { error_behavior: ErrorBehavior::Halt, ..Default::default() };
        let null = ExecuteOptions { error_behavior: ErrorBehavior::Null, ..Default::default() };
        let (halted, _) = single(make_world(users, posts, &failures, &[]), &doc, variables(flag), halt);
        let (nulled, _) = single(make_world(users, posts, &failures, &[]), &doc, variables(flag), null);
        if halted.get("data").is_none() {
            return Ok(()); // request error
        }
        let null_errors = sorted_errors(&nulled);
        if null_errors.is_empty() {
            prop_assert_eq!(&halted, &nulled);
        } else {
            prop_assert_eq!(halted["data"].clone(), Value::Null);
            let errors = halted["errors"].as_array().unwrap();
            prop_assert_eq!(errors.len(), 1);
            prop_assert!(null_errors.contains(&errors[0]), "halt error {} not among null-mode errors {:?}", errors[0], null_errors);
        }
    }

    #[test]
    fn incremental_folds_to_disabled(doc in document(true), (users, posts, failures) in world(), yields in interleaving(), flag in any::<bool>(), capacity in stream_capacity()) {
        let enabled = ExecuteOptions { incremental: IncrementalDelivery::Enabled, ..Default::default() };
        let (payloads, _) = run_at_capacity(make_world(users, posts, &failures, &yields), &doc, variables(flag), enabled, capacity);
        prop_assume!(!has_failed_group(&payloads));
        let (disabled, _) = single(make_world(users, posts, &failures, &yields), &doc, variables(flag), ExecuteOptions::default());
        if payloads[0].get("data").is_none() {
            prop_assert!(disabled.get("data").is_none());
            return Ok(());
        }
        // Work beneath a propagated null is dropped, not delivered: outside the property.
        prop_assume!(!errors_under_null(&disabled));
        let folded = fold(&payloads);
        prop_assert_eq!(folded.get("data"), disabled.get("data"), "doc: {}\ncapacity: {:?}\npayloads: {:?}\ndisabled: {}", doc, capacity, payloads, disabled);
        prop_assert_eq!(error_keys(&folded), error_keys(&disabled), "doc: {}\npayloads: {:?}", doc, payloads);
        if let Some(last) = payloads.last()
            && payloads.len() > 1 {
                prop_assert_eq!(last.get("hasNext"), Some(&Value::Bool(false)));
            }
    }

    #[test]
    fn breadth_first_call_count(doc in document(false), (users, posts, failures) in world(), flag in any::<bool>()) {
        let (_, calls) = single(make_world(users, posts, &failures, &[]), &doc, variables(flag), ExecuteOptions::default());
        let (_, reference_calls) = reference(make_world(users, posts, &failures, &[]), &doc, variables(flag), ExecuteOptions::default());
        prop_assert!(calls.len() as u64 <= reference_calls, "bfs {} > reference {}", calls.len(), reference_calls);
    }

    #[test]
    fn deterministic(doc in document(true), (users, posts, failures) in world(), yields_a in interleaving(), yields_b in interleaving(), flag in any::<bool>(), capacity in stream_capacity()) {
        let enabled = ExecuteOptions { incremental: IncrementalDelivery::Enabled, ..Default::default() };
        let (a1, _) = run_at_capacity(make_world(users, posts, &failures, &yields_a), &doc, variables(flag), enabled, capacity);
        let (a2, _) = run_at_capacity(make_world(users, posts, &failures, &yields_a), &doc, variables(flag), enabled, capacity);
        let (b, _) = run_at_capacity(make_world(users, posts, &failures, &yields_b), &doc, variables(flag), enabled, capacity);
        prop_assert_eq!(&a1, &a2);
        prop_assert_eq!(&a1, &b, "interleaving changed the output\ndoc: {}\ncapacity: {:?}", doc, capacity);
    }

    #[test]
    fn depth_limit_is_exact(depth in 1..40u32, limit in 1..40u32) {
        let mut query = String::from("id");
        for _ in 0..depth {
            query = format!("friends {{ {query} }}");
        }
        let query = format!("{{ users {{ {query} }} }}");
        let schema = greem_compliance::schema::Schema::<World>::builder()
            .query::<greem_compliance::world::QueryRoot>()
            .mutation::<greem_compliance::world::MutationRoot>()
            .max_depth(limit)
            .build()
            .unwrap();
        let document = schema.parse(&query).unwrap();
        let output = futures::executor::block_on(schema.execute(
            greem::Roots { query: greem_compliance::world::QueryRoot, mutation: greem_compliance::world::MutationRoot },
            World::seeded(2, 0),
            greem::Operation { document: &document, operation_name: None, variables: Value::Null },
            ExecuteOptions::default(),
        ));
        let response: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
        let reference = futures::executor::block_on(greem_reference::execute(
            &schema,
            greem::Roots { query: greem_compliance::world::QueryRoot, mutation: greem_compliance::world::MutationRoot },
            World::seeded(2, 0),
            greem::Operation { document: &document, operation_name: None, variables: Value::Null },
            ExecuteOptions::default(),
        ));
        // users adds one level; each friends adds one.
        let rejected = depth + 1 > limit;
        prop_assert_eq!(response.get("data").is_none(), rejected, "depth {} limit {}: {}", depth, limit, response);
        prop_assert_eq!(reference.response.get("data").is_none(), rejected);
    }
}

#[test]
fn generator_sanity() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let mut ok = 0;
    let mut errors = 0;
    let mut with_data_errors = 0;
    for _ in 0..200 {
        let doc = document(true).new_tree(&mut runner).unwrap().current();
        let (payloads, _) = run(
            World::seeded(3, 2),
            &doc,
            variables(true),
            ExecuteOptions {
                incremental: IncrementalDelivery::Enabled,
                ..Default::default()
            },
        );
        if payloads[0].get("data").is_some() {
            ok += 1;
            if payloads[0].get("errors").is_some() {
                with_data_errors += 1;
            }
        } else {
            errors += 1;
            eprintln!("REQUEST ERROR for {doc}: {}", payloads[0]);
        }
    }
    eprintln!("ok={ok} request_errors={errors} with_errors={with_data_errors}");
    assert!(ok > 150, "too many invalid documents: {errors}");
}
