//! graphql-js v17.0.2 `src/execution/__tests__/lists-test.ts`, case by case in upstream order.
//!
//! Upstream builds `type Query { listField: <shape> }` per case and feeds it a
//! JS value; the area schema names one field per shape and container, so
//! every query reads `{ <field> }` where upstream reads `{ listField }`.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::lists::{IndexResolution, Source, World};
use greem_compliance::harness::{Area, Harness};
use serde_json::{Value, json};

/// describe('Execute: Accepts any iterable as list value')
mod accepts_any_iterable_as_list_value {
    use super::*;

    /// it('Accepts a Set as a List value')
    #[test]
    fn accepts_a_set_as_a_list_value() {
        let world = World {
            string_set: ["apple", "banana", "apple", "coconut"]
                .map(String::from)
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let (v, _) = world.single("{ stringSet }", Value::Null, ExecuteOptions::default());
        assert_response(
            &v,
            &json!({"data": {"stringSet": ["apple", "banana", "coconut"]}}),
        );
    }

    /// it('Accepts a Generator function as a List value')
    ///
    /// Upstream yields `'one'`, `2` and `true` and lets `String` coerce them;
    /// here the strings themselves, from a stream that is always ready.
    #[test]
    fn accepts_a_generator_function_as_a_list_value() {
        let world = World {
            streamed_strings: vec![
                Ok(Some("one".into())),
                Ok(Some("2".into())),
                Ok(Some("true".into())),
            ],
            ..Default::default()
        };
        let (v, _) = world.single(
            "{ streamedStrings }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({"data": {"streamedStrings": ["one", "2", "true"]}}),
        );
    }

    /// it('Accepts function arguments as a List value')
    #[test]
    fn accepts_function_arguments_as_a_list_value() {
        let world = World {
            boxed_strings: vec!["one".into(), "two".into()],
            ..Default::default()
        };
        let (v, _) = world.single("{ boxedStrings }", Value::Null, ExecuteOptions::default());
        assert_response(&v, &json!({"data": {"boxedStrings": ["one", "two"]}}));
    }

    // it('Does not accept (Iterable) String-literal as a List value')
    // Not ported, reason (iv): a string at a list position does not compile.

    // it('Does not call iterator `return` when iteration throws')
    // Not ported, reason (ii): asserts the iterator's `next` count and that its `return` is never called.
}

/// describe('Execute: Handles abrupt completion in synchronous iterables')
mod handles_abrupt_completion_in_synchronous_iterables {
    // it('drains the iterator when `next` throws')
    // Not ported, reason (ii): asserts how far `next` is driven and that `return` is not called when the iterator itself throws.

    // it('drains the iterator when a null bubbles up from a non-null item')
    // Not ported, reason (iv): the null item at `[Int!]` is a JS value; a Rust list of `i32` cannot hold one.

    // it('handles iterator errors with later pending promises without calling `return`')
    // Not ported, reason (ii): asserts no unhandled rejection and no `return` call once the iterator throws.

    // it('handles sync errors with later pending promises without calling `return`')
    // Not ported, reason (ii): asserts no unhandled rejection and no `return` call; the null item at `[String!]!` is reason (iv) besides.
}

/// describe('Execute: Accepts async iterables as list value')
mod accepts_async_iterables_as_list_value {
    use super::*;

    /// An async generator of strings: one yield before each item.
    fn async_strings(items: &[&str]) -> World {
        World {
            item_yields: 1,
            streamed_strings: items.iter().map(|s| Ok(Some(s.to_string()))).collect(),
            ..Default::default()
        }
    }

    /// it('Accepts an AsyncGenerator function as a List value')
    #[test]
    fn accepts_an_async_generator_function_as_a_list_value() {
        let (v, _) = async_strings(&["two", "4", "false"]).single(
            "{ streamedStrings }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({"data": {"streamedStrings": ["two", "4", "false"]}}),
        );
    }

    /// it('Handles an AsyncGenerator function that throws')
    ///
    /// A JS generator throwing after two items fails the whole list; a
    /// streamed source hands its error out as the third item's, so spec
    /// 6.4.3 nulls that item and keeps the two delivered before it.
    #[test]
    fn handles_an_async_generator_function_that_throws() {
        let world = World {
            item_yields: 1,
            streamed_strings: vec![Ok(Some("two".into())), Ok(Some("4".into())), Err("bad")],
            ..Default::default()
        };
        let (v, _) = world.single(
            "{ streamedStrings }",
            Value::Null,
            ExecuteOptions::default(),
        );
        // upstream: data.listField null with the error at ["listField"]:
        // the generator's throw is the list's failure in JS.
        assert_response(
            &v,
            &json!({
                "data": {"streamedStrings": ["two", "4", null]},
                "errors": [{
                    "message": "bad",
                    "locations": [{"line": 1, "column": 3}],
                    "path": ["streamedStrings", 2],
                }],
            }),
        );
    }

    /// it('Handles an AsyncGenerator function where an intermediate value triggers an error')
    ///
    /// Upstream yields `{}` at a `String` item; the value a scalar cannot
    /// represent here is NaN at a `Float` item, with greem's coercion error.
    #[test]
    fn handles_an_async_generator_function_where_an_intermediate_value_triggers_an_error() {
        let world = World {
            item_yields: 1,
            streamed_floats: vec![Some(2.0), Some(f64::NAN), Some(4.0)],
            ..Default::default()
        };
        let (v, _) = world.single("{ streamedFloats }", Value::Null, ExecuteOptions::default());
        assert_response(
            &v,
            &json!({
                "data": {"streamedFloats": [2.0, null, 4.0]},
                "errors": [{
                    "message": "Float cannot represent NaN",
                    "locations": [{"line": 1, "column": 3}],
                    "path": ["streamedFloats", 1],
                    "extensions": {"code": "FLOAT_NOT_FINITE"},
                }],
            }),
        );
    }

    /// it('Handles errors from `completeValue` in AsyncIterables')
    #[test]
    fn handles_errors_from_complete_value_in_async_iterables() {
        let world = World {
            item_yields: 1,
            streamed_floats: vec![Some(2.0), Some(f64::NAN)],
            ..Default::default()
        };
        let (v, _) = world.single("{ streamedFloats }", Value::Null, ExecuteOptions::default());
        assert_response(
            &v,
            &json!({
                "data": {"streamedFloats": [2.0, null]},
                "errors": [{
                    "message": "Float cannot represent NaN",
                    "locations": [{"line": 1, "column": 3}],
                    "path": ["streamedFloats", 1],
                    "extensions": {"code": "FLOAT_NOT_FINITE"},
                }],
            }),
        );
    }

    /// Upstream's `completeObjectList`: three objects, each yielded after a
    /// promise, with `index` resolving as given per object.
    fn object_list(index: [IndexResolution; 3]) -> World {
        World {
            item_yields: 1,
            objects: index.to_vec(),
            ..Default::default()
        }
    }

    fn resolves() -> IndexResolution {
        IndexResolution {
            yields: 1,
            error: None,
        }
    }

    fn rejects(message: &'static str) -> IndexResolution {
        IndexResolution {
            yields: 1,
            error: Some(message),
        }
    }

    /// it('Handles promises from `completeValue` in AsyncIterables')
    #[test]
    fn handles_promises_from_complete_value_in_async_iterables() {
        let (v, _) = object_list([resolves(), resolves(), resolves()]).single(
            "{ streamedObjects { index } }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {"streamedObjects": [{"index": "0"}, {"index": "1"}, {"index": "2"}]},
            }),
        );
    }

    /// it('Handles rejected promises from `completeValue` in AsyncIterables')
    #[test]
    fn handles_rejected_promises_from_complete_value_in_async_iterables() {
        let (v, _) = object_list([resolves(), resolves(), rejects("bad")]).single(
            "{ streamedObjects { index } }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {"streamedObjects": [{"index": "0"}, {"index": "1"}, null]},
                "errors": [{
                    "message": "bad",
                    "locations": [{"line": 1, "column": 21}],
                    "path": ["streamedObjects", 2, "index"],
                }],
            }),
        );
    }

    /// it('handles mixture of sync and async errors in AsyncIterables')
    ///
    /// `index` rejects after two ticks for the first object and throws at
    /// once for the others. Each is a execution error raised while executing
    /// `index`, so each is added to `errors` (spec 6.4.4); the non-null items
    /// then null the list.
    #[test]
    fn handles_mixture_of_sync_and_async_errors_in_async_iterables() {
        let world = object_list([
            IndexResolution {
                yields: 2,
                error: Some("bad"),
            },
            IndexResolution {
                yields: 0,
                error: Some("also bad"),
            },
            IndexResolution {
                yields: 0,
                error: Some("also bad"),
            },
        ]);
        let (v, _) = world.single(
            "{ streamedNonNullObjects { index } }",
            Value::Null,
            ExecuteOptions::default(),
        );
        // upstream: only "also bad" at index 1: the first sync throw nulls
        // the list and the later rejection and the other throw are dropped.
        assert_response(
            &v,
            &json!({
                "data": {"streamedNonNullObjects": null},
                "errors": [
                    {
                        "message": "bad",
                        "locations": [{"line": 1, "column": 28}],
                        "path": ["streamedNonNullObjects", 0, "index"],
                    },
                    {
                        "message": "also bad",
                        "locations": [{"line": 1, "column": 28}],
                        "path": ["streamedNonNullObjects", 1, "index"],
                    },
                    {
                        "message": "also bad",
                        "locations": [{"line": 1, "column": 28}],
                        "path": ["streamedNonNullObjects", 2, "index"],
                    },
                ],
            }),
        );
    }

    /// it('Handles nulls yielded by async generator')
    ///
    /// The `[Int]` and `[Int]!` runs; the `[Int!]` and `[Int!]!` runs are
    /// reason (iv), a stream of `i32` cannot yield a null item.
    #[test]
    fn handles_nulls_yielded_by_async_generator() {
        let items = || Source::Items(vec![Ok(Some(1)), Ok(None), Ok(Some(2))]);
        let world = World {
            item_yields: 1,
            nullable_list_of_nullable: items(),
            non_null_list_of_nullable: items(),
            ..Default::default()
        };
        let (v, _) = world.clone().single(
            "{ streamedNullableListOfNullable }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({"data": {"streamedNullableListOfNullable": [1, null, 2]}}),
        );
        let (v, _) = world.single(
            "{ streamedNonNullListOfNullable }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({"data": {"streamedNonNullListOfNullable": [1, null, 2]}}),
        );
    }

    // it('Returns async iterable when list nulls')
    // Not ported, reason (ii): asserts the async iterator's `return` is called once; the null item at `[Int!]` is reason (iv) besides.

    // it('Ignores error on return method when async iterator nulls')
    // Not ported, reason (ii): the subject is a rejecting `return`.
}

/// describe('Execute: Handles list nullability')
mod handles_list_nullability {
    use super::*;

    /// One `[Int]` shape: its owned field and its streamed twin.
    struct Shape {
        owned: &'static str,
        streamed: &'static str,
    }

    /// A shape with upstream's `listField` written into its world field.
    struct Case {
        world: World,
        shape: Shape,
        is_list: bool,
    }

    fn nullable_list_of_nullable(source: Source<Option<i32>>) -> Case {
        Case {
            is_list: source.is_list(),
            world: World {
                nullable_list_of_nullable: source,
                ..Default::default()
            },
            shape: Shape {
                owned: "nullableListOfNullable",
                streamed: "streamedNullableListOfNullable",
            },
        }
    }

    fn non_null_list_of_nullable(source: Source<Option<i32>>) -> Case {
        Case {
            is_list: source.is_list(),
            world: World {
                non_null_list_of_nullable: source,
                ..Default::default()
            },
            shape: Shape {
                owned: "nonNullListOfNullable",
                streamed: "streamedNonNullListOfNullable",
            },
        }
    }

    fn nullable_list_of_non_null(source: Source<i32>) -> Case {
        Case {
            is_list: source.is_list(),
            world: World {
                nullable_list_of_non_null: source,
                ..Default::default()
            },
            shape: Shape {
                owned: "nullableListOfNonNull",
                streamed: "streamedNullableListOfNonNull",
            },
        }
    }

    fn non_null_list_of_non_null(source: Source<i32>) -> Case {
        Case {
            is_list: source.is_list(),
            world: World {
                non_null_list_of_non_null: source,
                ..Default::default()
            },
            shape: Shape {
                owned: "nonNullListOfNonNull",
                streamed: "streamedNonNullListOfNonNull",
            },
        }
    }

    /// Upstream's `complete`: the owned field as the value, then after a
    /// yield (`Promise<Array<T>>`); for a list also the streamed twin with a
    /// yield per item (`Array<Promise<T>>`), then after a yield as well
    /// (`Promise<Array<Promise<T>>>`). Every run must match `expected` for
    /// the field it read.
    fn complete(case: Case, expected: impl Fn(&str) -> Value) {
        let Case {
            world,
            shape,
            is_list,
        } = case;
        let mut runs = vec![(shape.owned, 0), (shape.owned, 1)];
        if is_list {
            runs.extend([(shape.streamed, 0), (shape.streamed, 1)]);
        }
        for (field, yields) in runs {
            let world = World {
                harness: Harness {
                    yields: vec![yields],
                    ..Default::default()
                },
                item_yields: 1,
                ..world.clone()
            };
            let (v, _) = world.single(
                &format!("{{ {field} }}"),
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(&v, &expected(field));
        }
    }

    /// The error `bad` raised at `path`, from the field at column 3.
    fn bad_at(path: Value) -> Value {
        json!({
            "message": "bad",
            "locations": [{"line": 1, "column": 3}],
            "path": path,
        })
    }

    /// it('Contains values')
    #[test]
    fn contains_values() {
        let nullable = || Source::Items(vec![Ok(Some(1)), Ok(Some(2))]);
        let non_null = || Source::Items(vec![Ok(1), Ok(2)]);
        let values = |field: &str| json!({"data": {field: [1, 2]}});
        complete(nullable_list_of_nullable(nullable()), values);
        complete(non_null_list_of_nullable(nullable()), values);
        complete(nullable_list_of_non_null(non_null()), values);
        complete(non_null_list_of_non_null(non_null()), values);
    }

    /// it('Contains null')
    ///
    /// The `[Int!]` and `[Int!]!` runs are reason (iv): a list of `i32`
    /// cannot hold the null item.
    #[test]
    fn contains_null() {
        let nullable = || Source::Items(vec![Ok(Some(1)), Ok(None), Ok(Some(2))]);
        let values = |field: &str| json!({"data": {field: [1, null, 2]}});
        complete(nullable_list_of_nullable(nullable()), values);
        complete(non_null_list_of_nullable(nullable()), values);
    }

    /// it('Returns null')
    ///
    /// The `[Int]!` and `[Int!]!` runs are reason (iv): a non-null list
    /// field cannot return null.
    #[test]
    fn returns_null() {
        let null = |field: &str| json!({"data": {field: null}});
        complete(nullable_list_of_nullable(Source::Null), null);
        complete(nullable_list_of_non_null(Source::Null), null);
    }

    /// it('Contains error')
    #[test]
    fn contains_error() {
        let nullable = || Source::Items(vec![Ok(Some(1)), Err("bad"), Ok(Some(2))]);
        let non_null = || Source::Items(vec![Ok(1), Err("bad"), Ok(2)]);
        let item_error = |field: &str| bad_at(json!([field, 1]));
        complete(
            nullable_list_of_nullable(nullable()),
            |field| json!({"data": {field: [1, null, 2]}, "errors": [item_error(field)]}),
        );
        complete(
            non_null_list_of_nullable(nullable()),
            |field| json!({"data": {field: [1, null, 2]}, "errors": [item_error(field)]}),
        );
        complete(
            nullable_list_of_non_null(non_null()),
            |field| json!({"data": {field: null}, "errors": [item_error(field)]}),
        );
        complete(
            non_null_list_of_non_null(non_null()),
            |field| json!({"data": null, "errors": [item_error(field)]}),
        );
    }

    /// it('Results in error')
    #[test]
    fn results_in_error() {
        let field_error = |field: &str| bad_at(json!([field]));
        complete(
            nullable_list_of_nullable(Source::Fail("bad")),
            |field| json!({"data": {field: null}, "errors": [field_error(field)]}),
        );
        complete(
            non_null_list_of_nullable(Source::Fail("bad")),
            |field| json!({"data": null, "errors": [field_error(field)]}),
        );
        complete(
            nullable_list_of_non_null(Source::Fail("bad")),
            |field| json!({"data": {field: null}, "errors": [field_error(field)]}),
        );
        complete(
            non_null_list_of_non_null(Source::Fail("bad")),
            |field| json!({"data": null, "errors": [field_error(field)]}),
        );
    }
}
