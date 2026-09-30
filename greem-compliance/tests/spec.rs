//! Hand-written spec-algorithm tests: expected JSON written from the spec,
//! never accepted from a snapshot. Grouped by spec section.

mod common;

use common::*;
use greem::{ErrorBehavior, ExecuteOptions, IncrementalDelivery};
use greem_compliance::world::{Failure, World};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn world(users: u32, posts: u32) -> World {
    World::seeded(users, posts)
}

fn failing(users: u32, posts: u32, failures: &[(&'static str, &'static str, u32)]) -> World {
    let failures: BTreeSet<Failure> = failures
        .iter()
        .map(|&(type_name, field, object)| Failure {
            type_name,
            field,
            object,
        })
        .collect();
    World {
        failures,
        ..World::seeded(users, posts)
    }
}

fn vars() -> Value {
    json!({"flag": true, "patch": {"name": "v", "email": null}})
}

// ---- 6.4 Executing fields: breadth-first call counts ----------------------

#[test]
fn set_based_call_count_is_independent_of_object_count() {
    let query = "{ users { name posts { title author { name } } } }";
    let mut counts = Vec::new();
    for users in [1, 2, 4] {
        let (v, calls) = single(
            world(users, 2),
            query,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert!(v.get("errors").is_none());
        counts.push(calls.len());
        // One call per field per non-empty scope: users, name, posts, title, author, author.name.
        assert_eq!(
            calls.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            [
                "Query.users",
                "User.name",
                "User.posts",
                "Post.title",
                "Post.author",
                "User.name"
            ]
        );
    }
    assert_eq!(counts, [6, 6, 6]);
    let (_, reference_calls) =
        reference(world(4, 2), query, Value::Null, ExecuteOptions::default());
    assert!(reference_calls > 6, "reference {reference_calls}");
}

#[test]
fn partition_arms_form_separate_scopes() {
    let query = r#"{ search { __typename ... on User { name } ... on Post { title } } }"#;
    let (v, calls) = single(world(2, 1), query, Value::Null, ExecuteOptions::default());
    assert_eq!(
        v["data"]["search"],
        json!([
            {"__typename": "User", "name": "user0"},
            {"__typename": "Post", "title": "post 0 of user0"},
            {"__typename": "User", "name": "user1"},
            {"__typename": "Post", "title": "post 0 of user1"}
        ])
    );
    // Both users resolve in one scope, both posts in another.
    assert_eq!(
        calls,
        vec![("Query.search", 1), ("User.name", 2), ("Post.title", 2)]
    );
}

// ---- 6.4.4 Handling execution errors ----------------------------------------

#[test]
fn non_null_error_propagates_to_nearest_nullable_ancestor() {
    let (v, _) = single(
        failing(2, 1, &[("Post", "title", 100)]),
        "{ users { name posts { title } } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    // Post.title of user1's post 0 fails: posts: [Post!]! and users: [User!]! are non-null, so data is null.
    assert_eq!(
        v,
        json!({"data": null, "errors": [{"message": "Post.title failed for 100", "locations": [{"line": 1, "column": 24}], "path": ["users", 1, "posts", 0, "title"]}]})
    );
    let (v, _) = single(
        failing(2, 1, &[("User", "email", 0)]),
        "{ users { name email } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        v,
        json!({
            "data": {"users": [{"name": "user0", "email": null}, {"name": "user1", "email": null}]},
            "errors": [{"message": "User.email failed for 0", "locations": [{"line": 1, "column": 16}], "path": ["users", 0, "email"]}]
        })
    );
}

#[test]
fn nested_list_levels_of_mixed_nullability() {
    // matrix: [[Int]!] -- outer nullable, inner lists non-null, items nullable.
    let (v, _) = single(
        world(3, 0),
        "{ matrix }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v, json!({"data": {"matrix": [[], [0], [0, null]]}}));
}

#[test]
fn error_behavior_modes() {
    let query = "{ users { name email } }";
    let world = || failing(2, 0, &[("User", "name", 1), ("User", "email", 0)]);
    let (propagate, _) = single(world(), query, Value::Null, ExecuteOptions::default());
    assert_eq!(propagate["data"], Value::Null);
    assert_eq!(propagate["errors"].as_array().unwrap().len(), 2);
    let (null, _) = single(
        world(),
        query,
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    assert_eq!(
        null["data"],
        json!({"users": [{"name": "user0", "email": null}, {"name": null, "email": null}]})
    );
    assert_eq!(null["errors"].as_array().unwrap().len(), 2);
    let (halt, _) = single(
        world(),
        query,
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            ..Default::default()
        },
    );
    assert_eq!(halt["data"], Value::Null);
    let errors = halt["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    // Deterministic: field order then object order within the scope.
    assert_eq!(errors[0]["path"], json!(["users", 1, "name"]));
}

#[test]
fn errors_are_ordered_by_generation_then_field_then_object() {
    let (v, _) = single(
        failing(
            2,
            1,
            &[
                ("User", "email", 0),
                ("User", "email", 1),
                ("Post", "title", 0),
                ("Post", "title", 100),
            ],
        ),
        "{ users { posts { title } email } }",
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    let paths: Vec<Value> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].clone())
        .collect();
    assert_eq!(
        paths,
        vec![
            json!(["users", 0, "email"]),
            json!(["users", 1, "email"]),
            json!(["users", 0, "posts", 0, "title"]),
            json!(["users", 1, "posts", 0, "title"])
        ]
    );
}

#[test]
fn errors_within_a_generation_are_field_then_object() {
    let (v, _) = single(
        failing(
            2,
            0,
            &[
                ("User", "name", 0),
                ("User", "name", 1),
                ("User", "email", 0),
                ("User", "email", 1),
            ],
        ),
        "{ users { name email } }",
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    let paths: Vec<Value> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].clone())
        .collect();
    assert_eq!(
        paths,
        vec![
            json!(["users", 0, "name"]),
            json!(["users", 1, "name"]),
            json!(["users", 0, "email"]),
            json!(["users", 1, "email"])
        ]
    );
}

#[test]
fn deferred_errors_are_field_then_object_too() {
    let options = ExecuteOptions {
        error_behavior: ErrorBehavior::Null,
        incremental: IncrementalDelivery::Enabled,
    };
    let (payloads, _) = run(
        failing(
            2,
            0,
            &[
                ("User", "name", 0),
                ("User", "name", 1),
                ("User", "email", 0),
                ("User", "email", 1),
            ],
        ),
        "{ users { id } ... @defer { users { name email } } }",
        Value::Null,
        options,
    );
    let errors: Vec<Value> = payloads[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .flat_map(|e| e["errors"].as_array().cloned().unwrap_or_default())
        .map(|e| e["path"].clone())
        .collect();
    assert_eq!(
        errors,
        vec![
            json!(["users", 0, "name"]),
            json!(["users", 1, "name"]),
            json!(["users", 0, "email"]),
            json!(["users", 1, "email"])
        ],
        "{payloads:?}"
    );
}

#[test]
fn initial_stream_error_at_a_non_null_item_does_not_wait_for_more() {
    // [Int!]!: the first item fails and the source never yields again; the
    // request must still answer, with the list nulled by propagation.
    let (payloads, _) = run(
        world(1, 0),
        "{ stuck @stream(initialCount: 2) }",
        Value::Null,
        incremental(),
    );
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert_eq!(payloads[0]["data"], Value::Null);
    assert_eq!(payloads[0]["errors"][0]["path"], json!(["stuck", 0]));
    assert!(payloads[0].get("hasNext").is_none());
}

#[test]
fn non_finite_floats_propagate_like_other_errors() {
    // score: Float (nullable) -> null with an error; a non-null Float would propagate.
    let (v, _) = single(
        World {
            nan_score: true,
            ..World::seeded(1, 0)
        },
        "{ users { score } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"], json!({"users": [{"score": null}]}));
    assert_eq!(
        v["errors"][0]["extensions"]["code"],
        json!("FLOAT_NOT_FINITE")
    );
    let (r, _) = reference(
        World {
            nan_score: true,
            ..World::seeded(1, 0)
        },
        "{ users { score } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_equivalent(&v, &r);
}

#[test]
fn cardinality_failure_is_a_framework_error_per_parent() {
    let world = World {
        cardinality_failure: true,
        ..World::seeded(2, 1)
    };
    let (v, _) = single(
        world,
        "{ users { posts { title } } }",
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    let errors = v["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["extensions"]["code"], json!("CARDINALITY"));
    assert_eq!(errors[1]["path"], json!(["users", 1, "posts"]));
}

// ---- 6.2.2 Mutations: serial roots -------------------------------------------

#[test]
fn mutation_roots_run_serially_and_stop_on_propagated_null() {
    let (v, calls) = single(
        world(2, 0),
        r#"mutation { a: rename(id: "0", name: "x") { name } fail b: rename(id: "1", name: "y") { name } }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"], Value::Null);
    assert_eq!(v["errors"][0]["path"], json!(["fail"]));
    // `b` never ran.
    assert_eq!(
        calls.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        ["Mutation.rename", "User.name", "Mutation.fail"]
    );
    let (v, calls) = single(
        world(2, 0),
        r#"mutation { a: rename(id: "0", name: "x") { name } fail b: rename(id: "1", name: "y") { name } }"#,
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    assert_eq!(
        v["data"],
        json!({"a": {"name": "user0"}, "fail": null, "b": {"name": "user1"}})
    );
    assert_eq!(calls.len(), 5);
}

#[test]
fn serial_mutation_errors_follow_execution_order() {
    // rename → email → fail: a root field's errors, its subtree's included,
    // precede the next root field's.
    let (v, _) = single(
        failing(1, 0, &[("User", "email", 0)]),
        r#"mutation { rename(id: "0", name: "x") { email } fail }"#,
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    let paths: Vec<&Value> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["path"])
        .collect();
    assert_eq!(
        paths,
        [&json!(["rename", "email"]), &json!(["fail"])],
        "{v}"
    );
}

// ---- 6.4.1 Coercing field arguments ------------------------------------------

#[test]
fn custom_scalar_variables_are_validated_before_execution() {
    // Variable coercion is a request error: nothing runs, not even the
    // mutation root ahead of the field the variable feeds.
    let good = "12340000-0000-0000-0000-000000000000";
    for (declaration, usage, variables, at) in [
        ("$x: UUID!", "id: $x", json!({"x": "nope"}), "$x"),
        (
            "$t: [UUID!]",
            "id: \"12340000-0000-0000-0000-000000000000\", tags: $t",
            json!({"t": [good, "nope"]}),
            "$t",
        ),
        (
            "$e: StampInput",
            "id: \"12340000-0000-0000-0000-000000000000\", extra: $e",
            json!({"e": {"other": 7}}),
            "$e",
        ),
    ] {
        let query = format!(
            "mutation({declaration}) {{ rename(id: \"0\", name: \"x\") {{ id }} stamp({usage}) }}"
        );
        let (payloads, calls) = run(world(1, 0), &query, variables, ExecuteOptions::default());
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].get("data").is_none(), "{query}: {payloads:?}");
        let message = payloads[0]["errors"][0]["message"].as_str().unwrap();
        assert!(message.contains(at), "{query}: {message}");
        assert!(payloads[0]["errors"][0].get("path").is_none(), "{query}");
        assert!(calls.is_empty(), "{query}: {calls:?}");
    }
    // Valid values and nulls at nullable positions pass.
    let (v, _) = single(
        world(1, 0),
        "mutation($x: UUID!, $t: [UUID!], $e: StampInput) { stamp(id: $x, tags: $t, extra: $e) }",
        json!({"x": good, "t": [good], "e": {"other": null}}),
        ExecuteOptions::default(),
    );
    assert_eq!(v, json!({"data": {"stamp": good}}));
}

#[test]
fn explicit_null_at_a_non_null_list_is_an_error() {
    // An input field `[Int]! = []` allows a nullable variable; null must not
    // become `[null]`.
    for (query, variables) in [
        (
            "query($v: [Int]) { count(bag: {values: $v}) }",
            json!({"v": null}),
        ),
        (
            "query($b: [JSON]) { count(bag: {blobs: $b}) }",
            json!({"b": null}),
        ),
    ] {
        let (v, calls) = single(world(1, 0), query, variables, ExecuteOptions::default());
        assert_eq!(v["data"], Value::Null, "{query}: {v}");
        assert!(
            v["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("expected a list, found null"),
            "{query}: {v}"
        );
        assert!(calls.is_empty(), "{query}: {calls:?}");
    }
    // Omitted variables take the default; single values still coerce to one item.
    let (v, _) = single(
        world(1, 0),
        "query($v: [Int], $b: [JSON]) { a: count(bag: {values: $v, blobs: $b}) b: count(bag: {values: 7, blobs: [null, 1]}) }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"], json!({"a": 0, "b": 3}), "{v}");
}

#[test]
fn argument_coercion() {
    let (v, _) = single(
        world(3, 0),
        "{ users { name } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["users"].as_array().unwrap().len(), 3);
    let (v, _) = single(
        world(3, 0),
        "query($n: Int) { users(first: $n) { name } }",
        json!({"n": 1}),
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["users"].as_array().unwrap().len(), 1);
    // Absent variable falls back to the schema default.
    let (v, _) = single(
        world(3, 0),
        "query($n: Int) { users(first: $n) { name } }",
        json!({}),
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["users"].as_array().unwrap().len(), 3);
    // Enum literal vs string: a string where an enum is expected is a validation error.
    let (v, _) = single(
        world(1, 0),
        r#"{ echo(input: {role: "ADMIN"}) { role } }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert!(v.get("data").is_none());
    let (v, _) = single(
        world(1, 0),
        r#"{ echo(input: {role: ADMIN}) { role } }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["echo"]["role"], json!("ADMIN"));
    // Enum through a variable is re-tagged as an enum value.
    let (v, _) = single(
        world(1, 0),
        "query($p: UserPatch!) { echo(input: $p) { role name } }",
        json!({"p": {"role": "MEMBER", "name": "q"}}),
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["echo"], json!({"role": "MEMBER", "name": "q"}));
}

#[test]
fn maybe_distinguishes_absent_null_and_value() {
    let (v, _) = single(
        world(1, 0),
        r#"{ echo(input: {}) { name email role } }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        v["data"]["echo"],
        json!({"name": null, "email": null, "role": "MEMBER"})
    );
    let (v, _) = single(
        world(1, 0),
        r#"{ echo(input: {name: null, email: "e"}) { name email } }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["echo"], json!({"name": "<null>", "email": "e"}));
}

#[test]
fn custom_scalar_codecs_round_trip() {
    let (v, _) = single(
        world(2, 0),
        "{ users { uuid } json(value: {a: [1, 2.5, \"x\", null], b: true}) }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        v["data"]["users"][1]["uuid"],
        json!("12340000-0000-0000-0000-000000000001")
    );
    assert_eq!(
        v["data"]["json"],
        json!({"a": [1, 2.5, "x", null], "b": true})
    );
    let (v, _) = single(
        world(1, 0),
        "query($j: JSON) { json(value: $j) }",
        json!({"j": {"nested": [true]}}),
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["json"], json!({"nested": [true]}));
    // Integers above i64::MAX keep their precision, as a literal and as a variable.
    let (v, _) = single(
        world(1, 0),
        "query($j: JSON) { a: json(value: 18446744073709551615) b: json(value: $j) }",
        json!({"j": [u64::MAX]}),
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"], json!({"a": u64::MAX, "b": [u64::MAX]}));
}

#[test]
fn a_custom_scalar_completing_to_null_at_a_non_null_position_is_an_error() {
    // strictJson: JSON!. JSON null is a value the Rust type allows and the
    // schema position does not.
    let query = "{ strictJson a: strictJson(value: 1) }";
    let null_error = |v: &Value| {
        assert_eq!(v["errors"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(v["errors"][0]["path"], json!(["strictJson"]), "{v}");
        assert_eq!(
            v["errors"][0]["extensions"]["code"],
            json!("NULL_AT_NON_NULL"),
            "{v}"
        );
    };
    for behavior in [ErrorBehavior::Propagate, ErrorBehavior::Halt] {
        let options = ExecuteOptions {
            error_behavior: behavior,
            ..Default::default()
        };
        let (v, _) = single(world(1, 0), query, Value::Null, options);
        assert_eq!(v["data"], Value::Null, "{behavior:?}: {v}");
        null_error(&v);
    }
    let options = ExecuteOptions {
        error_behavior: ErrorBehavior::Null,
        ..Default::default()
    };
    let (v, _) = single(world(1, 0), query, Value::Null, options);
    assert_eq!(v["data"], json!({"strictJson": null, "a": 1}), "{v}");
    null_error(&v);
    // The reference executor agrees.
    let (bfs, _) = single(world(1, 0), query, Value::Null, ExecuteOptions::default());
    let (dfs, _) = reference(world(1, 0), query, Value::Null, ExecuteOptions::default());
    assert_equivalent(&bfs, &dfs);
    // The nullable position still accepts JSON null.
    let (v, _) = single(
        world(1, 0),
        "{ json }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v, json!({"data": {"json": null}}));
}

#[test]
fn int_range_check_on_output() {
    // score is Float; Int range checks live in the greem unit tests. Here: a
    // Float output and an ID from an integer.
    let (v, _) = single(
        world(3, 0),
        "{ users { id score } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        v["data"]["users"],
        json!([{"id": "0", "score": null}, {"id": "1", "score": 1.5}, {"id": "2", "score": 3.0}])
    );
}

// ---- 3.x Abstract types ------------------------------------------------------

#[test]
fn interfaces_sub_interfaces_and_unions() {
    let query = r#"{
        nodes(ids: ["0", "post:0:0", "zzz"]) {
            __typename id
            ... on Resource { owner { name } }
            ... on User { role }
        }
    }"#;
    let (v, _) = single(world(1, 1), query, Value::Null, ExecuteOptions::default());
    assert_eq!(
        v["data"]["nodes"],
        json!([
            {"__typename": "User", "id": "0", "role": "ADMIN"},
            {"__typename": "Post", "id": "post:0:0", "owner": {"name": "user0"}},
            null
        ])
    );
}

// ---- Lookbehind planning hints ----------------------------------------------

#[test]
fn group_reading_hints_are_outside_the_fold_property_by_design() {
    // Ticket 11 lets a hint shape a field's result and lets its writer read the
    // delivery group; ticket 13's fold property therefore holds only for
    // documents whose writers ignore the group. `User.tag` is such a writer.
    let query = "{ users { ... @defer { tag } } }";
    // Disabled: the tree has no delivery groups, both executors see Initial.
    let (disabled, _) = single(world(1, 0), query, Value::Null, ExecuteOptions::default());
    assert_eq!(disabled["data"]["users"][0]["tag"], "initial");
    let (r, _) = reference(world(1, 0), query, Value::Null, ExecuteOptions::default());
    assert_equivalent(&disabled, &r);
    // Enabled: the writer sees Deferred, the accepting field answers differently,
    // and neither of the other two preconditions is violated.
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert!(!has_failed_group(&payloads));
    assert!(!errors_under_null(&disabled));
    let folded = fold(&payloads);
    assert_eq!(folded["data"]["users"][0]["tag"], "deferred");
    assert_ne!(folded["data"], disabled["data"]);
    // Without a deferred fragment around the writer, the two agree again.
    let plain = "{ users { tag } }";
    let (folded, _) = run(world(1, 0), plain, Value::Null, incremental());
    let (disabled, _) = single(world(1, 0), plain, Value::Null, ExecuteOptions::default());
    assert_eq!(fold(&folded)["data"], disabled["data"]);
    assert_eq!(disabled["data"]["users"][0]["tag"], "initial");
}

// (Planning hints are exercised by greem-macros/tests/object.rs, which asserts
// that `Post.author`'s hint reaches `User.posts` through `ctx.hint`.)

// ---- Introspection -----------------------------------------------------------

#[test]
fn introspection_rides_in_the_root_scope() {
    let (v, _) = single(
        world(1, 0),
        r#"{ __type(name: "Post") { name interfaces { name } } users { name } __typename }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        v["data"]["__type"],
        json!({"name": "Post", "interfaces": [{"name": "Node"}, {"name": "Resource"}]})
    );
    assert_eq!(v["data"]["users"], json!([{"name": "user0"}]));
    assert_eq!(v["data"]["__typename"], json!("Query"));
}

// ---- Incremental delivery ----------------------------------------------------

fn incremental() -> ExecuteOptions {
    ExecuteOptions {
        incremental: IncrementalDelivery::Enabled,
        ..Default::default()
    }
}

#[test]
fn defer_failed_group_reports_completed_errors_and_keeps_delivered_data() {
    let (p, _) = run(
        failing(2, 1, &[("Post", "title", 100)]),
        "{ users { name ... @defer { posts { title } } } }",
        Value::Null,
        incremental(),
    );
    assert_eq!(
        p[0]["data"],
        json!({"users": [{"name": "user0"}, {"name": "user1"}]})
    );
    assert_eq!(
        p[0]["pending"],
        json!([{"id": "0", "path": ["users", 0]}, {"id": "1", "path": ["users", 1]}])
    );
    let completed: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(completed[0], json!({"id": "0"}));
    assert_eq!(completed[1]["id"], json!("1"));
    assert_eq!(
        completed[1]["errors"][0]["path"],
        json!(["users", 1, "posts", 0, "title"])
    );
    let incremental: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        incremental,
        vec![json!({"id": "0", "data": {"posts": [{"title": "post 0 of user0"}]}})]
    );
}

#[test]
fn stream_non_null_item_error_terminates_the_stream() {
    let (p, _) = run(
        failing(1, 3, &[("User", "drafts", 0)]),
        "{ users { drafts @stream(initialCount: 1) { id } } }",
        Value::Null,
        incremental(),
    );
    assert_eq!(
        p[0]["data"],
        json!({"users": [{"drafts": [{"id": "post:0:0"}]}]})
    );
    let last = p.last().unwrap();
    let completed: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0]["errors"][0]["path"],
        json!(["users", 0, "drafts", 1])
    );
    assert_eq!(last["hasNext"], json!(false));
}

#[test]
fn halt_inside_a_deferred_group_leaves_others_alive() {
    let options = ExecuteOptions {
        error_behavior: ErrorBehavior::Halt,
        incremental: IncrementalDelivery::Enabled,
    };
    let (p, _) = run(
        failing(2, 1, &[("Post", "title", 100)]),
        "{ users { name ... @defer { posts { title } } } }",
        Value::Null,
        options,
    );
    assert_eq!(
        p[0]["data"],
        json!({"users": [{"name": "user0"}, {"name": "user1"}]})
    );
    let completed: Vec<Value> = p[1..]
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(completed[0], json!({"id": "0"}));
    assert_eq!(completed[1]["errors"].as_array().unwrap().len(), 1);
}

#[test]
fn nulled_parent_drops_deferred_work() {
    // name failure nulls users[1] (users: [User!]! is non-null, so data is null): nothing else ships.
    let (p, _) = run(
        failing(2, 1, &[("User", "name", 1)]),
        "{ users { name ... @defer { posts { title } } } }",
        Value::Null,
        incremental(),
    );
    assert_eq!(p[0]["data"], Value::Null);
    assert_eq!(p.len(), 1);
    assert!(p[0].get("pending").is_none());
    // A nullable position: only the surviving object's group is announced.
    let (p, _) = run(
        failing(2, 1, &[("User", "name", 1)]),
        r#"{ nodes(ids: ["0", "1"]) { ... on User { name ... @defer { posts { title } } } } }"#,
        Value::Null,
        incremental(),
    );
    assert_eq!(p[0]["data"], json!({"nodes": [{"name": "user0"}, null]}));
    assert_eq!(p[0]["pending"], json!([{"id": "0", "path": ["nodes", 0]}]));
}

#[test]
fn nullable_stream_item_errors_keep_the_source_alive() {
    // [Int]!: an item error nulls that item and the stream continues.
    for initial in [0, 2] {
        let query = format!("{{ numbers(fail: 1) @stream(initialCount: {initial}) }}");
        let (payloads, _) = run(world(1, 0), &query, Value::Null, incremental());
        assert!(!has_failed_group(&payloads), "{payloads:?}");
        let folded = fold(&payloads);
        assert_eq!(
            folded["data"],
            json!({"numbers": [1, null, 3]}),
            "{payloads:?}"
        );
        assert_eq!(
            error_keys(&folded),
            vec![(
                "[\"numbers\",1]".to_owned(),
                "\"number 1 failed\"".to_owned()
            )]
        );
        let (disabled, _) = single(world(1, 0), &query, Value::Null, ExecuteOptions::default());
        assert_eq!(disabled["data"], folded["data"]);
        assert_eq!(error_keys(&disabled), error_keys(&folded));
    }
}

#[test]
fn null_behavior_keeps_streaming_past_non_null_item_errors() {
    // drafts: [Post!]!: under Null nothing propagates, so a failed item is null
    // and the source continues, exactly as the Disabled run.
    let options = ExecuteOptions {
        error_behavior: ErrorBehavior::Null,
        incremental: IncrementalDelivery::Enabled,
    };
    let query = "{ users(first: 1) { drafts @stream(initialCount: 0) { id } } }";
    let (payloads, _) = run(
        failing(1, 3, &[("User", "drafts", 0)]),
        query,
        Value::Null,
        options,
    );
    assert!(!has_failed_group(&payloads), "{payloads:?}");
    let folded = fold(&payloads);
    assert_eq!(
        folded["data"],
        json!({"users": [{"drafts": [{"id": "post:0:0"}, null, {"id": "post:0:2"}]}]}),
        "{payloads:?}"
    );
    let (disabled, _) = single(
        failing(1, 3, &[("User", "drafts", 0)]),
        query,
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            ..Default::default()
        },
    );
    assert_eq!(disabled["data"], folded["data"]);
    assert_eq!(error_keys(&disabled), error_keys(&folded));
}

#[test]
fn nulled_parent_does_not_block_sibling_streams() {
    // nodes[0] is nulled by a name failure; nodes[1]'s stream still ships and completes.
    let query = r#"{ nodes(ids: ["0", "1"]) { ... on User { name drafts @stream(initialCount: 0) { id } } } }"#;
    let (payloads, _) = run(
        failing(2, 1, &[("User", "name", 0)]),
        query,
        Value::Null,
        incremental(),
    );
    let (disabled, _) = single(
        failing(2, 1, &[("User", "name", 0)]),
        query,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(
        disabled["data"],
        json!({"nodes": [null, {"name": "user1", "drafts": [{"id": "post:1:0"}]}]})
    );
    assert_eq!(fold(&payloads)["data"], disabled["data"], "{payloads:?}");
    assert_eq!(
        payloads[0]["pending"],
        json!([{"id": "0", "path": ["nodes", 1, "drafts"]}])
    );
    let completed: Vec<Value> = payloads
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(completed, vec![json!({"id": "0"})]);
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false));
}

#[test]
fn streamed_items_ship_in_list_order_per_parent() {
    // Capacity 1 makes one turn per item; the turns' subtrees finish at
    // different barriers and their slots are reused. Items still ship in order.
    let schema = greem_compliance::schema::Schema::<World>::builder()
        .query::<greem_compliance::world::QueryRoot>()
        .mutation::<greem_compliance::world::MutationRoot>()
        .stream_capacity(1)
        .build()
        .unwrap();
    let query =
        "{ users(first: 1) { drafts @stream(initialCount: 0) { id owner { friends { name } } } } }";
    let document = schema.parse(query).unwrap();
    let run = |options| -> Vec<Value> {
        futures::executor::block_on(schema.execute(
            greem::Roots {
                query: greem_compliance::world::QueryRoot,
                mutation: greem_compliance::world::MutationRoot,
            },
            World::seeded(2, 4),
            greem::Operation {
                document: &document,
                operation_name: None,
                variables: Value::Null,
            },
            options,
        ))
        .payloads
        .iter()
        .map(|p| serde_json::from_slice(&p.json).unwrap())
        .collect()
    };
    let payloads = run(incremental());
    let ids: Vec<Value> = payloads
        .iter()
        .flat_map(|p| p["incremental"].as_array().cloned().unwrap_or_default())
        .flat_map(|e| e["items"].as_array().cloned().unwrap_or_default())
        .map(|item| item["id"].clone())
        .collect();
    let disabled = run(ExecuteOptions::default());
    let expected: Vec<Value> = disabled[0]["data"]["users"][0]["drafts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].clone())
        .collect();
    assert_eq!(expected.len(), 4);
    assert_eq!(ids, expected, "{payloads:?}");
    assert_eq!(fold(&payloads)["data"], disabled[0]["data"]);
}

#[test]
fn work_inside_streamed_items_survives_item_by_item_shipping() {
    // Found by the property generator once root lists could stream.
    let variables = || json!({"flag": false});
    for (query, capacity) in [
        // A lazily drained list inside a streamed item holds the item's
        // group, which is not its to complete.
        (
            "{ users(first: 3) @stream(initialCount: 0) { drafts { id } } }",
            None,
        ),
        // Groups under an item that has not shipped yet wait for it instead
        // of being dropped as nulled.
        (
            "{ users(first: 3) @stream(initialCount: 0) { posts(first: 1) { id } drafts { ... @defer { id } } } }",
            Some(1),
        ),
        // A stream nested in a shipped item outlives that item's turn.
        (
            "query($flag: Boolean!) { users(first: 1) @stream(initialCount: 0) { drafts @stream(initialCount: 0) { id @include(if: $flag) } } }",
            None,
        ),
    ] {
        let (payloads, _) =
            run_at_capacity(world(3, 1), query, variables(), incremental(), capacity);
        let (disabled, _) = single(world(3, 1), query, variables(), ExecuteOptions::default());
        assert_eq!(
            fold(&payloads)["data"],
            disabled["data"],
            "{query}\n{payloads:?}"
        );
        assert_eq!(payloads.last().unwrap()["hasNext"], json!(false), "{query}");
        let ends = payloads
            .iter()
            .filter(|p| p["hasNext"] == json!(false))
            .count();
        assert_eq!(ends, 1, "{query}\n{payloads:?}");
    }
}

#[test]
fn reused_turn_slots_register_progress() {
    // With capacity 2 the third tag lands in a reused slot; it must still ship.
    let schema = greem_compliance::schema::Schema::<World>::builder()
        .query::<greem_compliance::world::QueryRoot>()
        .mutation::<greem_compliance::world::MutationRoot>()
        .stream_capacity(2)
        .build()
        .unwrap();
    let query = r#"{ node(id: "post:0:2") { ... on Post { tags @stream(initialCount: 0) } } }"#;
    let document = schema.parse(query).unwrap();
    let output = futures::executor::block_on(schema.execute(
        greem::Roots {
            query: greem_compliance::world::QueryRoot,
            mutation: greem_compliance::world::MutationRoot,
        },
        World::seeded(1, 3),
        greem::Operation {
            document: &document,
            operation_name: None,
            variables: Value::Null,
        },
        incremental(),
    ));
    let payloads: Vec<Value> = output
        .payloads
        .iter()
        .map(|p| serde_json::from_slice(&p.json).unwrap())
        .collect();
    assert_eq!(
        fold(&payloads)["data"],
        json!({"node": {"tags": ["t0", "t1", "t2"]}}),
        "{payloads:?}"
    );
    let completed: Vec<Value> = payloads
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(completed, vec![json!({"id": "0"})]);
}

#[test]
fn nested_streams_ship_outer_items_first() {
    let query = "{ users(first: 2) @stream(initialCount: 0) { id drafts @stream(initialCount: 0) { id } } }";
    let (payloads, _) = run(world(2, 1), query, Value::Null, incremental());
    let (disabled, _) = single(world(2, 1), query, Value::Null, ExecuteOptions::default());
    assert_eq!(fold(&payloads)["data"], disabled["data"], "{payloads:?}");
    let completed: Vec<Value> = payloads
        .iter()
        .flat_map(|p| p["completed"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        completed.len(),
        3,
        "users plus one drafts stream per user: {payloads:?}"
    );
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false));
}

#[test]
fn recursive_input_objects_round_trip() {
    let (v, _) = single(
        world(1, 0),
        r#"{ nest(filter: {not: {not: {term: "x"}}}) }"#,
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"], json!({"nest": 3}));
    let (v, _) = single(
        world(1, 0),
        "query($f: Filter!) { nest(filter: $f) }",
        json!({"f": {"not": {"all": [{"term": "y"}]}}}),
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"], json!({"nest": 2}));
}

#[test]
fn an_ended_stream_completes_without_waiting_for_other_streams() {
    // noisy(count: 0) ends at its first poll after release; stuck never yields
    // again (Null absorbs its error item). The empty stream's group must
    // complete on its own barrier, not when the stuck stream finally moves.
    let payloads = run_until_stalled(
        world(1, 0),
        "{ noisy(count: 0) @stream(initialCount: 0) stuck @stream(initialCount: 1) }",
        ExecuteOptions {
            error_behavior: ErrorBehavior::Null,
            incremental: IncrementalDelivery::Enabled,
        },
    );
    assert_eq!(
        payloads[0]["data"],
        json!({"noisy": [], "stuck": [null]}),
        "{payloads:?}"
    );
    let noisy = payloads[0]["pending"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["path"] == json!(["noisy"]))
        .map(|p| p["id"].clone())
        .expect("pending entry for noisy");
    let completed = payloads[1..].iter().any(|p| {
        p["completed"].as_array().is_some_and(|c| {
            c.iter()
                .any(|e| e["id"] == noisy && e.get("errors").is_none())
        })
    });
    assert!(completed, "{payloads:?}");
    assert!(
        payloads.iter().all(|p| p["hasNext"] == json!(true)),
        "{payloads:?}"
    );
}

#[test]
fn halt_reports_the_first_error_while_a_sibling_is_still_pending() {
    // name never resolves; email fails. Halt must not wait for name.
    let payloads = run_until_stalled(
        World {
            gate_field: Some("User.name"),
            ..failing(1, 0, &[("User", "email", 0)])
        },
        "{ users(first: 1) { name email } }",
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            ..Default::default()
        },
    );
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert_eq!(payloads[0]["data"], Value::Null);
    assert_eq!(payloads[0]["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        payloads[0]["errors"][0]["path"],
        json!(["users", 0, "email"])
    );
}

#[test]
fn incremental_directive_arguments_take_their_defaults() {
    // `if: Boolean! = true`: an unprovided variable means the default, an
    // explicit null is a request error.
    let defer = "query($c: Boolean) { users(first: 1) { id ... @defer(if: $c) { name } } }";
    let (payloads, _) = run(world(1, 0), defer, Value::Null, incremental());
    assert_eq!(payloads.len(), 2, "{payloads:?}");
    assert_eq!(payloads[0]["data"], json!({"users": [{"id": "0"}]}));
    let (payloads, _) = run(world(1, 0), defer, json!({"c": null}), incremental());
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert!(
        payloads[0]["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("\"if\" of non-null type \"Boolean!\" must not be null"),
        "{payloads:?}"
    );
    let stream = "query($c: Boolean, $n: Int) { users(first: 1) { drafts @stream(if: $c, initialCount: $n) { id } } }";
    let (payloads, _) = run(world(1, 2), stream, Value::Null, incremental());
    assert_eq!(
        payloads[0]["data"],
        json!({"users": [{"drafts": []}]}),
        "{payloads:?}"
    );
    assert!(payloads.len() > 1, "{payloads:?}");
}

#[test]
fn halt_keeps_the_error_recorded_inside_a_pending_column() {
    // stuck drains lazily inside its field future: item 0 fails, the source
    // then never yields. The error is recorded before the column completes.
    let payloads = run_until_stalled(
        world(1, 0),
        "{ stuck }",
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            ..Default::default()
        },
    );
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert_eq!(payloads[0]["data"], Value::Null);
    assert_eq!(
        payloads[0]["errors"][0]["path"],
        json!(["stuck", 0]),
        "{payloads:?}"
    );
}

#[test]
fn halt_ships_an_initial_pull_error_while_another_parent_still_pulls() {
    // users[0].drafts fails at item 1 inside the initial pull; users[1].drafts
    // never yields. The error must reach the barrier before that pull ends.
    let payloads = run_until_stalled(
        World {
            stalled_drafts: Some(1),
            ..failing(2, 2, &[("User", "drafts", 0)])
        },
        "{ users(first: 2) { drafts @stream(initialCount: 2) { id } } }",
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            incremental: IncrementalDelivery::Enabled,
        },
    );
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert_eq!(payloads[0]["data"], Value::Null);
    assert_eq!(
        payloads[0]["errors"][0]["path"],
        json!(["users", 0, "drafts", 1]),
        "{payloads:?}"
    );
}

#[test]
fn halt_stops_pulling_a_nullable_stream_at_the_first_error() {
    // numbers: [Int] absorbs item errors under other behaviors; under Halt the
    // source must not be polled again (it panics if it is).
    let options = ExecuteOptions {
        error_behavior: ErrorBehavior::Halt,
        incremental: IncrementalDelivery::Enabled,
    };
    let world = || World {
        panic_after_stream_error: true,
        ..world(1, 0)
    };
    let (payloads, _) = run(
        world(),
        "{ numbers(fail: 0) @stream(initialCount: 0) }",
        Value::Null,
        options,
    );
    let last = payloads.last().unwrap();
    assert_eq!(
        last["completed"][0]["errors"][0]["path"],
        json!(["numbers", 0]),
        "{payloads:?}"
    );
    let (payloads, _) = run(world(), "{ numbers(fail: 0) }", Value::Null, options);
    assert_eq!(payloads[0]["data"], Value::Null, "{payloads:?}");
    assert_eq!(payloads[0]["errors"][0]["path"], json!(["numbers", 0]));
}

#[test]
fn halt_does_not_pull_later_parents_of_the_halted_group() {
    // users[0].drafts fails at item 1; users[1].drafts panics if polled at all.
    for query in [
        "{ users(first: 2) { drafts @stream(initialCount: 2) { id } } }",
        "{ users(first: 2) { drafts { id } } }",
    ] {
        let (payloads, _) = run(
            World {
                panic_drafts: Some(1),
                ..failing(2, 2, &[("User", "drafts", 0)])
            },
            query,
            Value::Null,
            ExecuteOptions {
                error_behavior: ErrorBehavior::Halt,
                incremental: IncrementalDelivery::Enabled,
            },
        );
        assert_eq!(payloads.len(), 1, "{payloads:?}");
        assert_eq!(payloads[0]["data"], Value::Null);
        assert_eq!(
            payloads[0]["errors"][0]["path"],
            json!(["users", 0, "drafts", 1]),
            "{payloads:?}"
        );
    }
}

#[test]
fn halt_stops_pulling_at_an_item_its_scalar_cannot_represent() {
    // floats yields NaN, then never again. The NaN only fails at completion,
    // but Halt must not wait for the next item to find that out.
    for query in ["{ floats @stream(initialCount: 2) }", "{ floats }"] {
        let payloads = run_until_stalled(
            world(1, 0),
            query,
            ExecuteOptions {
                error_behavior: ErrorBehavior::Halt,
                incremental: IncrementalDelivery::Enabled,
            },
        );
        assert_eq!(payloads.len(), 1, "{query}: {payloads:?}");
        assert_eq!(payloads[0]["data"], Value::Null, "{query}");
        assert_eq!(
            payloads[0]["errors"][0]["path"],
            json!(["floats", 0]),
            "{query}: {payloads:?}"
        );
        assert_eq!(
            payloads[0]["errors"].as_array().unwrap().len(),
            1,
            "{query}"
        );
    }
}

#[test]
fn an_initial_pull_stops_at_an_item_a_non_null_position_cannot_represent() {
    // strictFloats yields NaN, then never again. At `[Float!]!` the NaN is a
    // terminal item error even though the source itself did not fail, so the
    // initial pull must not wait for a second item.
    let payloads = run_until_stalled(
        world(1, 0),
        "{ strictFloats @stream(initialCount: 2) }",
        incremental(),
    );
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert_eq!(payloads[0]["data"], Value::Null, "{payloads:?}");
    assert_eq!(
        payloads[0]["errors"][0]["path"],
        json!(["strictFloats", 0]),
        "{payloads:?}"
    );
    // At a nullable item type the error is absorbed and the pull goes on
    // waiting, as for any other item.
    let payloads = run_until_stalled(
        world(1, 0),
        "{ floats @stream(initialCount: 2) }",
        incremental(),
    );
    assert!(payloads.is_empty(), "{payloads:?}");
}

#[test]
fn nothing_is_streamed_beneath_a_nulled_list() {
    // team: [User!]. The first user's non-null name fails inside the initial
    // count, which nulls the list: there is no position left to stream into.
    let (payloads, _) = run(
        failing(3, 0, &[("User", "name", 0)]),
        "{ team @stream(initialCount: 1) { name } }",
        Value::Null,
        incremental(),
    );
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert_eq!(payloads[0]["data"], json!({"team": null}));
    assert_eq!(payloads[0]["errors"][0]["path"], json!(["team", 0, "name"]));
    assert!(payloads[0].get("pending").is_none(), "{payloads:?}");
    assert!(payloads[0].get("hasNext").is_none(), "{payloads:?}");
    // Intact, the rest of the list streams.
    let (payloads, _) = run(
        world(3, 0),
        "{ team @stream(initialCount: 1) { name } }",
        Value::Null,
        incremental(),
    );
    assert_eq!(
        fold(&payloads)["data"],
        json!({"team": [{"name": "user0"}, {"name": "user1"}, {"name": "user2"}]})
    );
}

#[test]
fn an_initial_pull_stops_at_a_custom_scalar_null_a_non_null_item_cannot_hold() {
    // strictJsons: [JSON!]! yields JSON null, then never again. The null is
    // only an error because of where it sits, and it must be found before the
    // pull waits for a second item.
    for behavior in [ErrorBehavior::Propagate, ErrorBehavior::Halt] {
        let options = ExecuteOptions {
            error_behavior: behavior,
            incremental: IncrementalDelivery::Enabled,
        };
        let payloads = run_until_stalled(
            world(1, 0),
            "{ strictJsons @stream(initialCount: 2) }",
            options,
        );
        assert_eq!(payloads.len(), 1, "{behavior:?}: {payloads:?}");
        assert_eq!(payloads[0]["data"], Value::Null, "{behavior:?}");
        assert_eq!(
            payloads[0]["errors"][0]["path"],
            json!(["strictJsons", 0]),
            "{behavior:?}: {payloads:?}"
        );
        assert_eq!(
            payloads[0]["errors"][0]["extensions"]["code"],
            json!("NULL_AT_NON_NULL"),
            "{behavior:?}"
        );
        // At a nullable item type JSON null is a value: the pull waits for
        // its second item like any other.
        let payloads =
            run_until_stalled(world(1, 0), "{ jsons @stream(initialCount: 2) }", options);
        assert!(payloads.is_empty(), "{behavior:?}: {payloads:?}");
    }
}

#[test]
fn halted_stream_groups_fail_with_their_error() {
    let (payloads, _) = run(
        world(1, 0),
        "{ numbers(fail: 0) @stream(initialCount: 0) }",
        Value::Null,
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            incremental: IncrementalDelivery::Enabled,
        },
    );
    assert_eq!(payloads[0]["data"], json!({"numbers": []}), "{payloads:?}");
    let id = payloads[0]["pending"][0]["id"].clone();
    let failed = payloads[1..].iter().any(|p| {
        p["completed"].as_array().is_some_and(|c| {
            c.iter()
                .any(|e| e["id"] == id && e["errors"][0]["path"] == json!(["numbers", 0]))
        })
    });
    assert!(failed, "{payloads:?}");
    assert!(
        payloads.iter().all(|p| p.get("incremental").is_none()),
        "{payloads:?}"
    );
    // A halted stream group ships past a pending sibling inside its items.
    let payloads = run_until_stalled(
        World {
            gate_field: Some("Post.id"),
            ..failing(1, 1, &[("Post", "title", 0)])
        },
        "{ users(first: 1) { drafts @stream(initialCount: 0) { id title } } }",
        ExecuteOptions {
            error_behavior: ErrorBehavior::Halt,
            incremental: IncrementalDelivery::Enabled,
        },
    );
    let id = payloads[0]["pending"][0]["id"].clone();
    let failed = payloads[1..].iter().any(|p| {
        p["completed"].as_array().is_some_and(|c| {
            c.iter().any(|e| {
                e["id"] == id && e["errors"][0]["path"] == json!(["users", 0, "drafts", 0, "title"])
            })
        })
    });
    assert!(failed, "{payloads:?}");
}

#[test]
fn a_fragment_spread_repeated_across_merged_fields_is_collected_once() {
    // Both `users` occurrences merge into one node; F is collected once there,
    // so its deferred fragment yields one group per user, not two.
    let query = "{ users(first: 2) { ...F } users(first: 2) { ...F } } fragment F on User { id ... @defer { name } }";
    let (payloads, _) = run(world(2, 0), query, Value::Null, incremental());
    let pending: Vec<&Value> = payloads
        .iter()
        .filter_map(|p| p["pending"].as_array())
        .flatten()
        .collect();
    assert_eq!(pending.len(), 2, "{payloads:?}");
    let completed: Vec<&Value> = payloads
        .iter()
        .filter_map(|p| p["completed"].as_array())
        .flatten()
        .collect();
    assert_eq!(completed.len(), 2, "{payloads:?}");
    assert_eq!(
        fold(&payloads)["data"],
        json!({"users": [{"id": "0", "name": "user0"}, {"id": "1", "name": "user1"}]})
    );
    // The same fragment reached under a defer and immediately is collected
    // for both contexts: `name` stays in the initial payload.
    let query = "{ users(first: 1) { ... @defer { ...F } ...F } } fragment F on User { name }";
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert_eq!(
        payloads[0]["data"],
        json!({"users": [{"name": "user0"}]}),
        "{payloads:?}"
    );
    // A spread that is itself deferred is a usage every time it appears.
    let query = "{ users(first: 1) { ...F @defer ...F @defer } } fragment F on User { name }";
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert_eq!(
        payloads[0]["pending"].as_array().unwrap().len(),
        2,
        "{payloads:?}"
    );
}

#[test]
fn nested_defers_under_a_streamed_item_are_delivered_before_the_end() {
    // The outer fragment has no fields of its own: its group completes at a
    // barrier, which releases the inner one. That release is progress.
    let query =
        "{ users(first: 1) @stream(initialCount: 0) { ... @defer { ... @defer { name } } } }";
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert_eq!(
        fold(&payloads)["data"],
        json!({"users": [{"name": "user0"}]}),
        "{payloads:?}"
    );
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false));
    let has_next_false = payloads
        .iter()
        .filter(|p| p["hasNext"] == json!(false))
        .count();
    assert_eq!(has_next_false, 1, "{payloads:?}");
    // The same chain of field-less fragments without a stream.
    let query = "{ users(first: 1) { ... @defer { ... @defer { ... @defer { name } } } } }";
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert_eq!(
        fold(&payloads)["data"],
        json!({"users": [{"name": "user0"}]}),
        "{payloads:?}"
    );
}

#[test]
fn a_defer_nested_in_stream_items_depends_on_its_own_enclosing_fragment() {
    // A and B both select the streamed drafts; C is nested under A only.
    // A fails (name is non-null), so C is dropped even though B completes.
    let query = "{ users(first: 1) { id \
        ... @defer(label: \"A\") { name drafts @stream(initialCount: 0) { ... @defer(label: \"C\") { title } } } \
        ... @defer(label: \"B\") { drafts @stream(initialCount: 0) { id } } } }";
    let (payloads, _) = run(
        failing(1, 1, &[("User", "name", 0)]),
        query,
        Value::Null,
        incremental(),
    );
    let text = serde_json::to_string(&payloads).unwrap();
    assert!(
        !text.contains("title") && !text.contains("post 0 of"),
        "{payloads:?}"
    );
    assert!(text.contains("post:0:0"), "{payloads:?}");
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false), "{text}");
    // Every announced group is resolved.
    let count = |key: &str| -> usize {
        payloads
            .iter()
            .filter_map(|p| p[key].as_array())
            .map(Vec::len)
            .sum()
    };
    assert_eq!(count("pending"), count("completed"), "{payloads:?}");
    // A deeper A fails a barrier after the items announced C: the client was
    // told to expect C, so it completes with A's error instead of vanishing.
    let late = query.replace(
        "{ name drafts",
        "{ posts(first: 1) { author { friends { friends { friends { id } } } name } } drafts",
    );
    let (payloads, _) = run(
        failing(3, 1, &[("User", "name", 0)]),
        &late,
        Value::Null,
        incremental(),
    );
    let pending_c = payloads
        .iter()
        .filter_map(|p| p["pending"].as_array())
        .flatten()
        .find(|p| p["label"] == json!("C"))
        .expect("C is announced with the items");
    let completed_c = payloads
        .iter()
        .filter_map(|p| p["completed"].as_array())
        .flatten()
        .find(|c| c["id"] == pending_c["id"])
        .expect("C is completed");
    assert_eq!(
        completed_c["errors"][0]["path"],
        json!(["users", 0, "posts", 0, "author", "name"]),
        "{payloads:?}"
    );
    assert!(!serde_json::to_string(&payloads).unwrap().contains("title"));
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false));
    // With A intact, C delivers.
    let (payloads, _) = run(world(1, 1), query, Value::Null, incremental());
    assert_eq!(
        fold(&payloads)["data"]["users"][0]["drafts"][0]["title"],
        json!("post 0 of user0"),
        "{payloads:?}"
    );
}

#[test]
fn a_field_shared_by_two_fragments_survives_one_of_them_failing() {
    let completed = |payloads: &[Value]| -> Vec<(Value, bool)> {
        let labels: Vec<(Value, Value)> = payloads
            .iter()
            .filter_map(|p| p["pending"].as_array())
            .flatten()
            .map(|p| (p["id"].clone(), p["label"].clone()))
            .collect();
        let mut out: Vec<(Value, bool)> = payloads
            .iter()
            .filter_map(|p| p["completed"].as_array())
            .flatten()
            .filter_map(|c| {
                let label = labels.iter().find(|(id, _)| *id == c["id"])?.1.clone();
                (!label.is_null()).then(|| (label, c.get("errors").is_some()))
            })
            .collect();
        out.sort_by_key(|(label, _)| label.to_string());
        out
    };
    let name_fails = || failing(1, 0, &[("User", "name", 0)]);
    // `users` is selected by A and B; only A's own `bad` fails. Whichever
    // fragment comes first, and streamed or not, B still gets `users`.
    for query in [
        "{ ... @defer(label: \"B\") { users @stream(initialCount: 0) { id } } ... @defer(label: \"A\") { bad: users { name } users @stream(initialCount: 0) { id } } }",
        "{ ... @defer(label: \"A\") { bad: users { name } users @stream(initialCount: 0) { id } } ... @defer(label: \"B\") { users @stream(initialCount: 0) { id } } }",
        "{ ... @defer(label: \"B\") { users { id } } ... @defer(label: \"A\") { bad: users { name } users { id } } }",
        "{ ... @defer(label: \"A\") { bad: users { name } users { id } } ... @defer(label: \"B\") { users { id } } }",
    ] {
        let (payloads, _) = run(name_fails(), query, Value::Null, incremental());
        assert_eq!(
            fold(&payloads)["data"],
            json!({"users": [{"id": "0"}]}),
            "{query}\n{payloads:?}"
        );
        assert_eq!(
            completed(&payloads),
            [(json!("A"), true), (json!("B"), false)],
            "{query}\n{payloads:?}"
        );
        assert_eq!(payloads.last().unwrap()["hasNext"], json!(false), "{query}");
    }
    // The shared field itself failing fails every fragment that selects it.
    let query = "{ ... @defer(label: \"B\") { users { name } x: users { id } } ... @defer(label: \"A\") { users { name } } }";
    let (payloads, _) = run(name_fails(), query, Value::Null, incremental());
    assert_eq!(fold(&payloads)["data"], json!({}), "{payloads:?}");
    assert_eq!(
        completed(&payloads),
        [(json!("A"), true), (json!("B"), true)],
        "{payloads:?}"
    );
    // Nothing failing: the shared field ships once, with the first fragment.
    let query = "{ ... @defer(label: \"B\") { users { id } } ... @defer(label: \"A\") { bad: users { name } users { id } } }";
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert_eq!(
        fold(&payloads)["data"],
        json!({"users": [{"id": "0"}], "bad": [{"name": "user0"}]}),
        "{payloads:?}"
    );
    let shipped_users = payloads
        .iter()
        .filter_map(|p| p["incremental"].as_array())
        .flatten()
        .filter(|e| e["data"].get("users").is_some())
        .count();
    assert_eq!(shipped_users, 1, "{payloads:?}");
    assert_eq!(
        completed(&payloads),
        [(json!("A"), false), (json!("B"), false)],
        "{payloads:?}"
    );
}

#[test]
fn a_nested_defer_with_only_shared_fields_still_waits_for_its_enclosing_fragment() {
    // CA and CB share `email`, so neither has a field set of its own. CA's
    // fragment A completes early; CB's fragment B fails later. CB must not
    // report success on the strength of the shared data alone.
    let query = "{ users(first: 1) { id \
        ... @defer(label: \"A\") { friends @stream(initialCount: 0) { ... @defer(label: \"CA\") { email } } } \
        ... @defer(label: \"B\") { \
            posts(first: 1) { author { friends { friends { friends { id } } } name } } \
            friends @stream(initialCount: 0) { ... @defer(label: \"CB\") { email } } } } }";
    let (payloads, _) = run(
        failing(3, 1, &[("User", "name", 0)]),
        query,
        Value::Null,
        incremental(),
    );
    let outcomes = |label: &str| -> Vec<bool> {
        payloads
            .iter()
            .filter_map(|p| p["pending"].as_array())
            .flatten()
            .filter(|p| p["label"] == json!(label))
            .map(|pending| {
                payloads
                    .iter()
                    .filter_map(|p| p["completed"].as_array())
                    .flatten()
                    .find(|c| c["id"] == pending["id"])
                    .unwrap_or_else(|| panic!("{label} never completed: {payloads:?}"))
                    .get("errors")
                    .is_some()
            })
            .collect()
    };
    assert_eq!(outcomes("A"), [false], "{payloads:?}");
    assert_eq!(outcomes("B"), [true], "{payloads:?}");
    let ca = outcomes("CA");
    let cb = outcomes("CB");
    assert!(
        !ca.is_empty() && ca.iter().all(|failed| !failed),
        "{payloads:?}"
    );
    assert!(
        !cb.is_empty() && cb.iter().all(|failed| *failed),
        "{payloads:?}"
    );
    // A's copy still delivered the shared field.
    let friends = &fold(&payloads)["data"]["users"][0]["friends"];
    assert!(
        friends
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f.get("email").is_some()),
        "{payloads:?}"
    );
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false));
}

#[test]
fn a_nested_defer_failing_with_its_shared_fields_still_waits_for_its_enclosing_fragment() {
    // The shared `id` fails for friend 1, which fails CA and CB there. CB's
    // enclosing fragment B is still running: CB's failure waits for it too.
    let query = "{ users(first: 1) { uuid \
        ... @defer(label: \"A\") { friends @stream(initialCount: 0) { ... @defer(label: \"CA\") { id } } } \
        ... @defer(label: \"B\") { \
            posts(first: 1) { author { friends { friends { friends { uuid } } } } } \
            friends @stream(initialCount: 0) { ... @defer(label: \"CB\") { id } } } } }";
    let (payloads, _) = run(
        failing(3, 1, &[("User", "id", 1)]),
        query,
        Value::Null,
        incremental(),
    );
    // (payload index, failed) of every completion of a label.
    let completions = |label: &str| -> Vec<(usize, bool)> {
        payloads
            .iter()
            .filter_map(|p| p["pending"].as_array())
            .flatten()
            .filter(|p| p["label"] == json!(label))
            .map(|pending| {
                payloads
                    .iter()
                    .enumerate()
                    .find_map(|(i, p)| {
                        p["completed"]
                            .as_array()?
                            .iter()
                            .find(|c| c["id"] == pending["id"])
                            .map(|c| (i, c.get("errors").is_some()))
                    })
                    .unwrap_or_else(|| panic!("{label} never completed: {payloads:?}"))
            })
            .collect()
    };
    let b = completions("B");
    assert_eq!(b.len(), 1, "{payloads:?}");
    assert!(!b[0].1, "{payloads:?}");
    let cb = completions("CB");
    assert!(cb.iter().any(|(_, failed)| *failed), "{payloads:?}");
    assert!(cb.iter().all(|(at, _)| *at >= b[0].0), "{payloads:?}");
    assert_eq!(payloads.last().unwrap()["hasNext"], json!(false));
}

#[test]
fn a_reused_fragment_keeps_each_enclosing_defer_context() {
    // A and B both stream friends through F. F's deferred email depends on
    // A in one occurrence and on B in the other: A failing must not take
    // B's copy with it.
    let query = "{ users(first: 1) { id \
        ... @defer(label: \"A\") { name friends @stream(initialCount: 0) { ...F } } \
        ... @defer(label: \"B\") { friends @stream(initialCount: 0) { ...F } } } } \
        fragment F on User { id ... @defer(label: \"E\") { email } }";
    // The same operation with F expanded in place behaves the same.
    let inline = query[..query.find("fragment F").unwrap()]
        .replacen("...F", "id ... @defer(label: \"E1\") { email }", 1)
        .replacen("...F", "id ... @defer(label: \"E2\") { email }", 1);
    for query in [query, inline.as_str()] {
        let (payloads, _) = run(
            failing(2, 0, &[("User", "name", 0)]),
            query,
            Value::Null,
            incremental(),
        );
        let friends = &fold(&payloads)["data"]["users"][0]["friends"];
        let with_email = friends
            .as_array()
            .unwrap_or_else(|| panic!("{query}\n{payloads:?}"))
            .iter()
            .filter(|f| f.get("email").is_some())
            .count();
        assert!(with_email > 0, "{query}\n{payloads:?}");
        assert_eq!(payloads.last().unwrap()["hasNext"], json!(false), "{query}");
    }
}

#[test]
fn equivalent_stream_directives_merge() {
    // The same arguments in another order, or spelled through defaults.
    for options in [incremental(), ExecuteOptions::default()] {
        let (payloads, _) = run(
            world(1, 0),
            "{ numbers @stream(initialCount: 0, if: true) numbers @stream(if: true, initialCount: 0) numbers @stream }",
            Value::Null,
            options,
        );
        assert_eq!(
            fold(&payloads)["data"],
            json!({"numbers": [1, 2, 3]}),
            "{payloads:?}"
        );
    }
}

#[test]
fn disabled_delivery_ignores_stream_arguments_but_not_merge_conflicts() {
    let query = "{ numbers @stream(initialCount: -1) }";
    let (v, _) = single(world(1, 0), query, Value::Null, ExecuteOptions::default());
    assert_eq!(v["data"], json!({"numbers": [1, 2, 3]}), "{v}");
    let (payloads, _) = run(world(1, 0), query, Value::Null, incremental());
    assert_eq!(
        payloads[0]["errors"][0]["message"],
        json!("initialCount must be positive"),
        "{payloads:?}"
    );
    let (v, calls) = single(
        world(1, 0),
        "{ numbers @stream(initialCount: 1) numbers @stream(initialCount: 2) }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert!(
        v["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("differing stream directives"),
        "{v}"
    );
    assert!(calls.is_empty());
}

#[test]
fn stream_disabled_when_not_requested() {
    let (v, _) = single(
        world(1, 2),
        "{ users { drafts @stream(initialCount: 1) { id } } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert_eq!(v["data"]["users"][0]["drafts"].as_array().unwrap().len(), 2);
    assert!(v.get("pending").is_none());
}

// ---- Depth policy -------------------------------------------------------------

#[test]
fn depth_counts_composites_defer_and_stream() {
    // users(+1) { friends(+1) { name } } = 2; @defer around friends adds one; @stream on drafts adds one.
    let cases = [
        ("{ users { friends { name } } }", 2),
        ("{ users { ... @defer { friends { name } } } }", 3),
        ("{ users { ... @defer { ... @defer { name } } } }", 3),
        ("{ ... @defer { __typename } }", 1),
        ("{ users { drafts @stream(initialCount: 0) { title } } }", 3),
        ("{ users { posts { author { posts { title } } } } }", 4),
    ];
    for (query, depth) in cases {
        for limit in [depth - 1, depth] {
            let schema = greem_compliance::schema::Schema::<World>::builder()
                .query::<greem_compliance::world::QueryRoot>()
                .mutation::<greem_compliance::world::MutationRoot>()
                .max_depth(limit)
                .build()
                .unwrap();
            let document = schema.parse(query).unwrap();
            let output = futures::executor::block_on(schema.execute(
                greem::Roots {
                    query: greem_compliance::world::QueryRoot,
                    mutation: greem_compliance::world::MutationRoot,
                },
                World::seeded(1, 1),
                greem::Operation {
                    document: &document,
                    operation_name: None,
                    variables: Value::Null,
                },
                incremental(),
            ));
            let v: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
            assert_eq!(
                v.get("data").is_none(),
                limit < depth,
                "{query} at limit {limit}: {v}"
            );
        }
    }
}

#[test]
fn request_errors_have_no_data() {
    let (v, _) = single(
        world(1, 0),
        "{ nope }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert!(v.get("data").is_none());
    let (v, _) = single(
        world(1, 0),
        "query($id: ID!) { user(id: $id) { id } }",
        json!({}),
        ExecuteOptions::default(),
    );
    assert!(v.get("data").is_none());
    let (v, _) = single(
        world(1, 0),
        "subscription { users { id } }",
        Value::Null,
        ExecuteOptions::default(),
    );
    assert!(v.get("data").is_none());
}

#[test]
fn skip_include_and_fragments() {
    let query = r#"
        query Q($flag: Boolean!) {
            users { ...Bits name @skip(if: $flag) ... on User @include(if: $flag) { role } }
        }
        fragment Bits on User { id ...Deeper }
        fragment Deeper on Node { id }
    "#;
    let (v, _) = single(world(1, 0), query, vars(), ExecuteOptions::default());
    assert_eq!(v["data"], json!({"users": [{"id": "0", "role": "ADMIN"}]}));
    let (r, _) = reference(world(1, 0), query, vars(), ExecuteOptions::default());
    assert_equivalent(&v, &r);
}
