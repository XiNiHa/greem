//! graphql-js v17.0.2 `src/execution/incremental/__tests__/stream-test.ts`, case by case in upstream order.
//!
//! Where upstream ships one payload per item or batches items as its
//! promises happen to settle, greem makes one stream turn per generation
//! from what the pump buffered (bounded by the stream capacity) and ships
//! it at the next barrier; a nested group's data never shares the payload
//! that delivers its parent. Cases whose sequence upstream shows per item
//! run at capacity 1 and say so; the rest run at the default.

use crate::common::*;
use greem::Error;
use greem_compliance::graphql_js::stream::{
    DeeperNestedObject, Friend, NestedObject, Source, World, friend,
};
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// Upstream's `friends`, as nullable items.
fn friends() -> Vec<Result<Option<Friend>, Error>> {
    (0..3).map(|i| Ok(Some(friend(i)))).collect()
}

fn strings(values: &[&str]) -> Vec<Result<Option<String>, Error>> {
    values.iter().map(|v| Ok(Some((*v).to_owned()))).collect()
}

fn string_lists(values: &[&str]) -> Vec<Result<Option<Vec<Option<String>>>, Error>> {
    values
        .iter()
        .map(|v| Ok(Some(vec![Some((*v).to_owned()); 3])))
        .collect()
}

/// describe('Execute: stream directive')
mod stream_directive {
    use super::*;

    /// it('Can stream a list field')
    #[test]
    fn can_stream_a_list_field() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "{ scalarList @stream(initialCount: 1) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarList": ["apple"]},
                    "pending": [{"id": "0", "path": ["scalarList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": ["banana", "coconut"]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Does not call `return` on an exhausted sync iterator')
    // Not ported, reason (ii): asserts the iterator's `return` is not called and how far it
    // was advanced; the payload sequence is 'Can stream a list field'.

    /// it('Can use default value of initialCount')
    #[test]
    fn can_use_default_value_of_initial_count() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run("{ scalarList @stream }", Value::Null, incremental());
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarList": []},
                    "pending": [{"id": "0", "path": ["scalarList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": ["apple", "banana", "coconut"]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Negative values of initialCount throw field errors')
    ///
    /// greem raises the error when it coerces the field's arguments, so the
    /// resolver never runs and the nullable list is null.
    #[test]
    fn negative_values_of_initial_count_throw_field_errors() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "{ scalarList @stream(initialCount: -2) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "data": {"scalarList": null},
                "errors": [{
                    "message": "initialCount must not be negative",
                    "locations": [{"line": 1, "column": 3}],
                    "path": ["scalarList"],
                }],
            })],
        );
    }

    /// it('Returns label from stream directive')
    #[test]
    fn returns_label_from_stream_directive() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"{ scalarList @stream(initialCount: 1, label: "scalar-stream") }"#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarList": ["apple"]},
                    "pending": [{"id": "0", "path": ["scalarList"], "label": "scalar-stream"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": ["banana", "coconut"]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Treats null stream label the same as no label')
    #[test]
    fn treats_null_stream_label_the_same_as_no_label() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "{ scalarList @stream(initialCount: 1, label: null) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarList": ["apple"]},
                    "pending": [{"id": "0", "path": ["scalarList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": ["banana", "coconut"]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can disable @stream using if argument')
    #[test]
    fn can_disable_stream_using_if_argument() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "{ scalarList @stream(initialCount: 0, if: false) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({"data": {"scalarList": ["apple", "banana", "coconut"]}})],
        );
    }

    /// it('Does not disable stream with null if argument')
    #[test]
    fn does_not_disable_stream_with_null_if_argument() {
        let world = World {
            scalar_list: Source::List(strings(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "query ($shouldStream: Boolean) { scalarList @stream(initialCount: 2, if: $shouldStream) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarList": ["apple", "banana"]},
                    "pending": [{"id": "0", "path": ["scalarList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": ["coconut"]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can stream multi-dimensional lists')
    #[test]
    fn can_stream_multi_dimensional_lists() {
        let world = World {
            scalar_list_list: Source::List(string_lists(&["apple", "banana", "coconut"])),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "{ scalarListList @stream(initialCount: 1) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarListList": [["apple", "apple", "apple"]]},
                    "pending": [{"id": "0", "path": ["scalarListList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [
                            ["banana", "banana", "banana"],
                            ["coconut", "coconut", "coconut"],
                        ],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can stream a field that returns a list of promises')
    #[test]
    fn can_stream_a_field_that_returns_a_list_of_promises() {
        let world = World {
            friend_list: Source::Promises(friends()),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 2) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "friendList": [
                            {"name": "Luke", "id": "1"},
                            {"name": "Han", "id": "2"},
                        ],
                    },
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can stream in correct order with lists of promises')
    ///
    /// At capacity 1 each turn carries one item, upstream's one payload per
    /// promise; at the default the three ready items make one turn.
    #[test]
    fn can_stream_in_correct_order_with_lists_of_promises() {
        let world = World {
            friend_list: Source::Promises(friends()),
            ..Default::default()
        };
        let (payloads, _) = world.run_at_capacity(
            r#"
      query {
        friendList @stream(initialCount: 0) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
            Some(1),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": []},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Luke", "id": "1"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Han", "id": "2"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Does not execute early if not specified')
    // Not ported, reason (iii): `enableEarlyExecution`.

    // it('Executes early if specified')
    // Not ported, reason (iii): `enableEarlyExecution`.

    /// it('Can stream a field that returns a list with nested promises')
    #[test]
    fn can_stream_a_field_that_returns_a_list_with_nested_promises() {
        let world = World {
            friend_list: Source::List(
                (0..3)
                    .map(|i| {
                        Ok(Some(Friend {
                            yields: 1,
                            ..friend(i)
                        }))
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 2) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "friendList": [
                            {"name": "Luke", "id": "1"},
                            {"name": "Han", "id": "2"},
                        ],
                    },
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles rejections in a field that returns a list of promises before initialCount is reached')
    #[test]
    fn handles_rejections_in_a_field_that_returns_a_list_of_promises_before_initial_count_is_reached()
     {
        let world = World {
            friend_list: Source::Promises(vec![
                Ok(Some(friend(0))),
                Err(Error::new("bad")),
                Ok(Some(friend(2))),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 2) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"name": "Luke", "id": "1"}, null]},
                    "errors": [{
                        "message": "bad",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["friendList", 1],
                    }],
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles rejections in a field that returns a list of promises after initialCount is reached')
    ///
    /// At capacity 1 the failed item and the last one make separate turns,
    /// upstream's sequence; at the default they share one.
    #[test]
    fn handles_rejections_in_a_field_that_returns_a_list_of_promises_after_initial_count_is_reached()
     {
        let world = World {
            friend_list: Source::Promises(vec![
                Ok(Some(friend(0))),
                Err(Error::new("bad")),
                Ok(Some(friend(2))),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run_at_capacity(
            r#"
      query {
        friendList @stream(initialCount: 1) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
            Some(1),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"name": "Luke", "id": "1"}]},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [null],
                        "errors": [{
                            "message": "bad",
                            "locations": [{"line": 3, "column": 9}],
                            "path": ["friendList", 1],
                        }],
                    }],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can stream a field that returns an async iterable')
    ///
    /// The source yields before each item, so each generation buffers one
    /// and each item ships in its own payload.
    /// upstream: `[Luke]`, then `[Han, Leia]` with the completion (both had
    /// settled by the time the first payload shipped).
    #[test]
    fn can_stream_a_field_that_returns_an_async_iterable() {
        let world = World {
            friend_list: Source::Iterable {
                items: friends(),
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": []},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Luke", "id": "1"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Han", "id": "2"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can stream multi-dimensional lists from async iterable')
    ///
    /// The inner lists are plain lists: only the outermost level streams.
    #[test]
    fn can_stream_multi_dimensional_lists_from_async_iterable() {
        let world = World {
            scalar_list_list: Source::Iterable {
                items: string_lists(&["apple", "banana", "coconut"]),
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "{ scalarListList @stream(initialCount: 1) }",
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"scalarListList": [["apple", "apple", "apple"]]},
                    "pending": [{"id": "0", "path": ["scalarListList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [["banana", "banana", "banana"]]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [["coconut", "coconut", "coconut"]]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can stream a field that returns an async iterable, using a non-zero initialCount')
    #[test]
    fn can_stream_a_field_that_returns_an_async_iterable_using_a_non_zero_initial_count() {
        let world = World {
            friend_list: Source::Iterable {
                items: friends(),
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 2) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "friendList": [
                            {"name": "Luke", "id": "1"},
                            {"name": "Han", "id": "2"},
                        ],
                    },
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"name": "Leia", "id": "3"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Negative values of initialCount throw field errors on a field that returns an async iterable')
    #[test]
    fn negative_values_of_initial_count_throw_field_errors_on_a_field_that_returns_an_async_iterable()
     {
        let world = World {
            friend_list: Source::Iterable {
                items: Vec::new(),
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: -2) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "data": {"friendList": null},
                "errors": [{
                    "message": "initialCount must not be negative",
                    "locations": [{"line": 3, "column": 9}],
                    "path": ["friendList"],
                }],
            })],
        );
    }

    // it('Does not execute early if not specified, when streaming from an async iterable')
    // Not ported, reason (iii): `enableEarlyExecution`.

    // it('Executes early if specified when streaming from an async iterable')
    // Not ported, reason (iii): `enableEarlyExecution`.

    // it('Can handle concurrent calls to .next() without waiting')
    // Not ported, reason (ii): concurrent `.next()` calls on the result iterator; the payload
    // sequence is 'Can stream a field that returns an async iterable, using a non-zero
    // initialCount'.

    /// it('Handles error thrown in async iterable before initialCount is reached')
    ///
    /// A source yielding `Err` is an item error, which the nullable item
    /// absorbs; the source is not polled past it until the stream is
    /// released, when it ends.
    /// upstream: the iterable throwing is a execution error at the list, one
    /// plain response `{errors: [{bad, path: [friendList]}], data: {friendList: null}}`.
    #[test]
    fn handles_error_thrown_in_async_iterable_before_initial_count_is_reached() {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![Ok(Some(friend(0))), Err(Error::new("bad"))],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 2) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"name": "Luke", "id": "1"}, null]},
                    "errors": [{
                        "message": "bad",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["friendList", 1],
                    }],
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles error thrown in async iterable after initialCount is reached')
    ///
    /// The `Err` item is nulled and the stream completes normally.
    /// upstream: `completed: [{id: "0", errors: [{bad, path: [friendList]}]}]`,
    /// the iterable's throw failing the stream.
    #[test]
    fn handles_error_thrown_in_async_iterable_after_initial_count_is_reached() {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![Ok(Some(friend(0))), Err(Error::new("bad"))],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 1) {
          name
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"name": "Luke", "id": "1"}]},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [null],
                        "errors": [{
                            "message": "bad",
                            "locations": [{"line": 3, "column": 9}],
                            "path": ["friendList", 1],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles null returned in non-null list items after initialCount is reached')
    ///
    /// The type encoding has no null at a non-null item: the item is an
    /// `Err` carrying upstream's message instead.
    #[test]
    fn handles_null_returned_in_non_null_list_items_after_initial_count_is_reached() {
        let world = World {
            non_null_friend_list: Source::List(vec![
                Ok(friend(0)),
                Err(Error::new(
                    "Cannot return null for non-nullable field Query.nonNullFriendList.",
                )),
                Ok(friend(1)),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nonNullFriendList @stream(initialCount: 1) {
          name
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nonNullFriendList": [{"name": "Luke"}]},
                    "pending": [{"id": "0", "path": ["nonNullFriendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": "Cannot return null for non-nullable field Query.nonNullFriendList.",
                            "locations": [{"line": 3, "column": 9}],
                            "path": ["nonNullFriendList", 1],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles null returned in non-null async iterable list items after initialCount is reached')
    ///
    /// The null item is an `Err` as above; the generator's `finally` throw
    /// is iterator plumbing with no claim here.
    #[test]
    fn handles_null_returned_in_non_null_async_iterable_list_items_after_initial_count_is_reached()
    {
        let world = World {
            non_null_friend_list: Source::Iterable {
                items: vec![
                    Ok(friend(0)),
                    Err(Error::new(
                        "Cannot return null for non-nullable field Query.nonNullFriendList.",
                    )),
                ],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nonNullFriendList @stream(initialCount: 1) {
          name
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nonNullFriendList": [{"name": "Luke"}]},
                    "pending": [{"id": "0", "path": ["nonNullFriendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": "Cannot return null for non-nullable field Query.nonNullFriendList.",
                            "locations": [{"line": 3, "column": 9}],
                            "path": ["nonNullFriendList", 1],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Drains sync iterators with later promises when null bubbles past the stream')
    // Not ported, reason (ii): asserts the iterator is drained without `return` and without an
    // unhandled rejection; the payload sequence is 'Handles null returned in non-null list
    // items after initialCount is reached'.

    // it('Handles errors thrown by completeValue after initialCount is reached')
    // Not ported, reason (iv): an object at a String position is rejected at compile time.

    /// it('Handles async errors thrown by completeValue after initialCount is reached')
    ///
    /// At capacity 1 the failed item and the last one make separate turns,
    /// upstream's sequence.
    #[test]
    fn handles_async_errors_thrown_by_complete_value_after_initial_count_is_reached() {
        let world = World {
            friend_list: Source::Promises(vec![
                Ok(Some(friend(0))),
                Ok(Some(Friend {
                    non_null_name: Err(Error::new("Oops")),
                    yields: 1,
                    ..friend(1)
                })),
                Ok(Some(friend(1))),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run_at_capacity(
            r#"
      query {
        friendList @stream(initialCount: 1) {
          nonNullName
        }
      }
    "#,
            Value::Null,
            incremental(),
            Some(1),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"nonNullName": "Luke"}]},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [null],
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["friendList", 1, "nonNullName"],
                        }],
                    }],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"nonNullName": "Han"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles nested async errors thrown by completeValue after initialCount is reached')
    ///
    /// A sync list whose fields are promises; capacity 1 as above.
    #[test]
    fn handles_nested_async_errors_thrown_by_complete_value_after_initial_count_is_reached() {
        let world = World {
            friend_list: Source::List(vec![
                Ok(Some(Friend {
                    yields: 1,
                    ..friend(0)
                })),
                Ok(Some(Friend {
                    non_null_name: Err(Error::new("Oops")),
                    yields: 1,
                    ..friend(1)
                })),
                Ok(Some(Friend {
                    yields: 1,
                    ..friend(1)
                })),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run_at_capacity(
            r#"
      query {
        friendList @stream(initialCount: 1) {
          nonNullName
        }
      }
    "#,
            Value::Null,
            incremental(),
            Some(1),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"nonNullName": "Luke"}]},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [null],
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["friendList", 1, "nonNullName"],
                        }],
                    }],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"nonNullName": "Han"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Stops late stream item completion after item null bubbling')
    // Not ported, reason (i): asserts a sibling field's resolver is never called once the item
    // nulled, on a promise the test resolves after the response ended; the payload sequence
    // is 'Handles async errors thrown by completeValue after initialCount is reached'.

    /// it('Handles async errors thrown by completeValue after initialCount is reached for a non-nullable list')
    #[test]
    fn handles_async_errors_thrown_by_complete_value_after_initial_count_is_reached_for_a_non_nullable_list()
     {
        let world = World {
            non_null_friend_list: Source::Promises(vec![
                Ok(friend(0)),
                Ok(Friend {
                    non_null_name: Err(Error::new("Oops")),
                    yields: 1,
                    ..friend(1)
                }),
                Ok(friend(1)),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nonNullFriendList @stream(initialCount: 1) {
          nonNullName
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nonNullFriendList": [{"nonNullName": "Luke"}]},
                    "pending": [{"id": "0", "path": ["nonNullFriendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["nonNullFriendList", 1, "nonNullName"],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles nested async errors thrown by completeValue after initialCount is reached for a non-nullable list')
    #[test]
    fn handles_nested_async_errors_thrown_by_complete_value_after_initial_count_is_reached_for_a_non_nullable_list()
     {
        let world = World {
            non_null_friend_list: Source::List(vec![
                Ok(Friend {
                    yields: 1,
                    ..friend(0)
                }),
                Ok(Friend {
                    non_null_name: Err(Error::new("Oops")),
                    yields: 1,
                    ..friend(1)
                }),
                Ok(Friend {
                    yields: 1,
                    ..friend(1)
                }),
            ]),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nonNullFriendList @stream(initialCount: 1) {
          nonNullName
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nonNullFriendList": [{"nonNullName": "Luke"}]},
                    "pending": [{"id": "0", "path": ["nonNullFriendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["nonNullFriendList", 1, "nonNullName"],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles async errors thrown by completeValue after initialCount is reached from async iterable')
    #[test]
    fn handles_async_errors_thrown_by_complete_value_after_initial_count_is_reached_from_async_iterable()
     {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![
                    Ok(Some(friend(0))),
                    Ok(Some(Friend {
                        non_null_name: Err(Error::new("Oops")),
                        yields: 1,
                        ..friend(1)
                    })),
                    Ok(Some(friend(1))),
                ],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 1) {
          nonNullName
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"nonNullName": "Luke"}]},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [null],
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["friendList", 1, "nonNullName"],
                        }],
                    }],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"nonNullName": "Han"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles async errors thrown by completeValue after initialCount is reached from async generator for a non-nullable list')
    #[test]
    fn handles_async_errors_thrown_by_complete_value_after_initial_count_is_reached_from_async_generator_for_a_non_nullable_list()
     {
        let world = World {
            non_null_friend_list: Source::Iterable {
                items: vec![
                    Ok(friend(0)),
                    Ok(Friend {
                        non_null_name: Err(Error::new("Oops")),
                        yields: 1,
                        ..friend(1)
                    }),
                ],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nonNullFriendList @stream(initialCount: 1) {
          nonNullName
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nonNullFriendList": [{"nonNullName": "Luke"}]},
                    "pending": [{"id": "0", "path": ["nonNullFriendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["nonNullFriendList", 1, "nonNullName"],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Handles async errors thrown by completeValue after initialCount is reached from async iterable for a non-nullable list when the async iterable does not provide a return method) ')
    // Not ported, reason (ii): the subject is an iterator without `return`; the payload
    // sequence is 'Handles async errors thrown by completeValue after initialCount is reached
    // from async generator for a non-nullable list'.

    // it('Handles async errors thrown by completeValue after initialCount is reached from async iterable for a non-nullable list when the async iterable provides concurrent next/return methods and has a slow return ')
    // Not ported, reason (ii): the subject is awaiting a slow `return`; same payload sequence
    // as above.

    /// it('Filters payloads that are nulled')
    ///
    /// The null at the non-null field is an `Err` carrying upstream's
    /// message; the stream's group is dropped at announcement since its
    /// parent object was nulled.
    #[test]
    fn filters_payloads_that_are_nulled() {
        let world = World {
            nested_object: Some(NestedObject {
                non_null_scalar_field: Err(Error::new(
                    "Cannot return null for non-nullable field NestedObject.nonNullScalarField.",
                )),
                nested_friend_list: Source::Iterable {
                    items: vec![Ok(Some(friend(0)))],
                    end_yields: 0,
                },
                yields: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nestedObject {
          nonNullScalarField
          nestedFriendList @stream(initialCount: 0) {
            name
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "data": {"nestedObject": null},
                "errors": [{
                    "message": "Cannot return null for non-nullable field NestedObject.nonNullScalarField.",
                    "locations": [{"line": 4, "column": 11}],
                    "path": ["nestedObject", "nonNullScalarField"],
                }],
            })],
        );
    }

    /// it('Filters payloads that are nulled by a later synchronous error')
    #[test]
    fn filters_payloads_that_are_nulled_by_a_later_synchronous_error() {
        let world = World {
            nested_object: Some(NestedObject {
                nested_friend_list: Source::Iterable {
                    items: vec![Ok(Some(friend(0)))],
                    end_yields: 0,
                },
                non_null_scalar_field: Err(Error::new(
                    "Cannot return null for non-nullable field NestedObject.nonNullScalarField.",
                )),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nestedObject {
          nestedFriendList @stream(initialCount: 0) {
            name
          }
          nonNullScalarField
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "data": {"nestedObject": null},
                "errors": [{
                    "message": "Cannot return null for non-nullable field NestedObject.nonNullScalarField.",
                    "locations": [{"line": 7, "column": 11}],
                    "path": ["nestedObject", "nonNullScalarField"],
                }],
            })],
        );
    }

    /// it('Does not filter payloads when null error is in a different path')
    ///
    /// The deferred set settles in the generation after release; the
    /// stream's first turn is made then and ships a barrier later.
    /// upstream: both entries in one payload.
    #[test]
    fn does_not_filter_payloads_when_null_error_is_in_a_different_path() {
        let world = World {
            nested_object: Some(NestedObject {
                scalar_field: Err(Error::new("Oops")),
                nested_friend_list: Source::Iterable {
                    items: vec![Ok(Some(friend(0)))],
                    end_yields: 0,
                },
                yields: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        otherNestedObject: nestedObject {
          ... @defer {
            scalarField
          }
        }
        nestedObject {
          nestedFriendList @stream(initialCount: 0) {
            name
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "otherNestedObject": {},
                        "nestedObject": {"nestedFriendList": []},
                    },
                    "pending": [
                        {"id": "0", "path": ["otherNestedObject"]},
                        {"id": "1", "path": ["nestedObject", "nestedFriendList"]},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "data": {"scalarField": null},
                        "errors": [{
                            "message": "Oops",
                            "locations": [{"line": 5, "column": 13}],
                            "path": ["otherNestedObject", "scalarField"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "1", "items": [{"name": "Luke"}]}],
                    "completed": [{"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Cancels async stream items when null bubbles past the stream')
    // Not ported, reason (ii): the subject is cancelling an in-flight item on promises the test
    // resolves by hand; the response is 'Filters payloads that are nulled by a later
    // synchronous error'.

    /// it('Filters stream payloads that are nulled in a deferred payload')
    #[test]
    fn filters_stream_payloads_that_are_nulled_in_a_deferred_payload() {
        let world = World {
            nested_object: Some(NestedObject {
                deeper_nested_object: Some(DeeperNestedObject {
                    non_null_scalar_field: Err(Error::new(
                        "Cannot return null for non-nullable field DeeperNestedObject.nonNullScalarField.",
                    )),
                    deeper_nested_friend_list: Source::Iterable {
                        items: vec![Ok(Some(friend(0)))],
                        end_yields: 0,
                    },
                    yields: 1,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nestedObject {
          ... @defer {
            deeperNestedObject {
              nonNullScalarField
              deeperNestedFriendList @stream(initialCount: 0) {
                name
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nestedObject": {}},
                    "pending": [{"id": "0", "path": ["nestedObject"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "data": {"deeperNestedObject": null},
                        "errors": [{
                            "message": "Cannot return null for non-nullable field DeeperNestedObject.nonNullScalarField.",
                            "locations": [{"line": 6, "column": 15}],
                            "path": ["nestedObject", "deeperNestedObject", "nonNullScalarField"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Filters defer payloads that are nulled in a stream response')
    ///
    /// The deferred fragment under the nulled item is never announced.
    #[test]
    fn filters_defer_payloads_that_are_nulled_in_a_stream_response() {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![Ok(Some(Friend {
                    non_null_name: Err(Error::new(
                        "Cannot return null for non-nullable field Friend.nonNullName.",
                    )),
                    yields: 1,
                    ..friend(0)
                }))],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
    query {
      friendList @stream(initialCount: 0) {
        nonNullName
        ... @defer {
          name
        }
      }
    }
  "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": []},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "items": [null],
                        "errors": [{
                            "message": "Cannot return null for non-nullable field Friend.nonNullName.",
                            "locations": [{"line": 4, "column": 9}],
                            "path": ["friendList", 0, "nonNullName"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Returns iterator and ignores errors when stream payloads are filtered')
    // Not ported, reason (ii): asserts `return` is called once and its rejection ignored; the
    // payload sequence is 'Filters stream payloads that are nulled in a deferred payload'.

    /// it('Handles promises returned by completeValue after initialCount is reached')
    #[test]
    fn handles_promises_returned_by_complete_value_after_initial_count_is_reached() {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![
                    Ok(Some(friend(0))),
                    Ok(Some(friend(1))),
                    Ok(Some(Friend {
                        yields: 1,
                        ..friend(2)
                    })),
                ],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream(initialCount: 1) {
          id
          name
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"id": "1", "name": "Luke"}]},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"id": "2", "name": "Han"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"id": "3", "name": "Leia"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles overlapping deferred and non-deferred streams')
    ///
    /// Upstream executes this document without validating it and notes
    /// that validation rejects it: the RFC's Field Selection Merging rule
    /// (`HasNoOverlappingStreams`) forbids merging two selections of a
    /// field when either carries `@stream`. greem validates at parse, so
    /// the spec outcome is a request error; the wording mirrors greem's
    /// existing differing-stream-directives error. greem only requires the
    /// merged directives to agree and executes the document instead.
    #[test]
    fn handles_overlapping_deferred_and_non_deferred_streams() {
        let world = World {
            nested_object: Some(NestedObject {
                nested_friend_list: Source::Iterable {
                    items: vec![Ok(Some(friend(0))), Ok(Some(friend(1)))],
                    end_yields: 0,
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nestedObject {
          nestedFriendList @stream(initialCount: 0) {
            id
          }
        }
        nestedObject {
          ... @defer {
            nestedFriendList @stream(initialCount: 0) {
              id
              name
              ... @defer {
                innerName: name
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "fields `nestedFriendList` conflict because they have overlapping stream directives",
                    "locations": [{"line": 4, "column": 11}],
                }],
            })],
        );
    }

    /// it('Re-promotes a completed stream when a slower sibling defer resolves later')
    ///
    /// The same invalid document shape as above (upstream says so): two
    /// selections of `nestedFriendList`, both streamed, so the spec outcome
    /// is a request error. greem executes it: both fragments settle at one
    /// barrier, since a generation waits for every future it polls, and the
    /// shared streamed field ships under the first fragment.
    #[test]
    fn re_promotes_a_completed_stream_when_a_slower_sibling_defer_resolves_later() {
        let world = World {
            nested_object: Some(NestedObject {
                scalar_field: Ok(Some("slow".to_owned())),
                nested_friend_list: Source::List(friends()),
                yields: 3,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nestedObject {
          ... @defer {
            nestedFriendList @stream { name }
          }
          ... @defer {
            scalarField
            nestedFriendList @stream { name }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "fields `nestedFriendList` conflict because they have overlapping stream directives",
                    "locations": [{"line": 5, "column": 13}],
                }],
            })],
        );
    }

    /// it('Returns payloads in correct order when parent deferred fragment resolves slower than stream')
    ///
    /// The stream is announced by the payload that delivers the fragment
    /// and released after it, so its items follow whatever the fragment's
    /// own fields took.
    #[test]
    fn returns_payloads_in_correct_order_when_parent_deferred_fragment_resolves_slower_than_stream()
    {
        let world = World {
            nested_object: Some(NestedObject {
                scalar_field: Ok(Some("slow".to_owned())),
                nested_friend_list: Source::Iterable {
                    items: vec![Ok(Some(friend(0))), Ok(Some(friend(1)))],
                    end_yields: 0,
                },
                yields: 3,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        nestedObject {
          ... DeferFragment @defer
        }
      }
      fragment DeferFragment on NestedObject {
        scalarField
        nestedFriendList @stream(initialCount: 0) {
          name
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"nestedObject": {}},
                    "pending": [{"id": "0", "path": ["nestedObject"]}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "1", "path": ["nestedObject", "nestedFriendList"]}],
                    "incremental": [{
                        "id": "0",
                        "data": {"scalarField": "slow", "nestedFriendList": []},
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "1", "items": [{"name": "Luke"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "1", "items": [{"name": "Han"}]}],
                    "completed": [{"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can @defer fields that are resolved after async iterable is complete')
    ///
    /// The source ends right after its second item, before either deferred
    /// fragment runs. Each item's fragment is announced with the item and
    /// released after its payload, so the first fragment's data ships with
    /// the second item, when the stream also completes; `completed` lists
    /// the fragments the barrier settled before the streams it shipped.
    /// upstream: `[Luke]` with `name: Luke`; `[Han]` with the stream
    /// completed; `name: Han`.
    #[test]
    fn can_defer_fields_that_are_resolved_after_async_iterable_is_complete() {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![
                    Ok(Some(friend(0))),
                    Ok(Some(Friend {
                        yields: 1,
                        ..friend(1)
                    })),
                ],
                end_yields: 0,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
    query {
      friendList @stream(label:"stream-label") {
        ...NameFragment @defer(label: "DeferName")
        id
      }
    }
    fragment NameFragment on Friend {
      name
    }
  "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": []},
                    "pending": [{"id": "0", "path": ["friendList"], "label": "stream-label"}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "1", "path": ["friendList", 0], "label": "DeferName"}],
                    "incremental": [{"id": "0", "items": [{"id": "1"}]}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "2", "path": ["friendList", 1], "label": "DeferName"}],
                    "incremental": [
                        {"id": "0", "items": [{"id": "2"}]},
                        {"id": "1", "data": {"name": "Luke"}},
                    ],
                    "completed": [{"id": "1"}, {"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "2", "data": {"name": "Han"}}],
                    "completed": [{"id": "2"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can @defer fields that are resolved before async iterable is complete')
    ///
    /// The source keeps yielding after its last item for more generations
    /// than the second item's fragment needs, so the stream completes last.
    /// The fragment under the initial item is announced after the stream,
    /// in tree order (upstream numbers it first).
    #[test]
    fn can_defer_fields_that_are_resolved_before_async_iterable_is_complete() {
        let world = World {
            friend_list: Source::Iterable {
                items: vec![Ok(Some(friend(0))), Ok(Some(friend(1)))],
                end_yields: 4,
            },
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
    query {
      friendList @stream(initialCount: 1, label:"stream-label") {
        ...NameFragment @defer(label: "DeferName")
        id
      }
    }
    fragment NameFragment on Friend {
      name
    }
  "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": [{"id": "1"}]},
                    "pending": [
                        {"id": "0", "path": ["friendList"], "label": "stream-label"},
                        {"id": "1", "path": ["friendList", 0], "label": "DeferName"},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "1", "data": {"name": "Luke"}}],
                    "completed": [{"id": "1"}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "2", "path": ["friendList", 1], "label": "DeferName"}],
                    "incremental": [{"id": "0", "items": [{"id": "2"}]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "2", "data": {"name": "Han"}}],
                    "completed": [{"id": "2"}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Returns underlying async iterables when returned generator is returned')
    // Not ported, reason (ii): `return()` on the result iterator; only the initial payload
    // is asserted.

    // it('Awaits stream source async iterable return before iterator return settles')
    // Not ported, reason (ii): the timing of `return()` on the result iterator.

    // it('Can return async iterable when underlying iterable does not have a return method')
    // Not ported, reason (ii): `return()` on the result iterator over a source without one.

    // it('Returns underlying async iterables when returned generator is thrown')
    // Not ported, reason (ii): `throw()` on the result iterator.

    // it('Returns underlying async iterables when resource is disposed before source completion')
    // Not ported, reason (ii): async disposal of the result iterator.

    // it('Does not return underlying async iterables when resource is disposed after source completion')
    // Not ported, reason (ii): asserts `return` is not called after disposal; the payload
    // sequence is 'Can use default value of initialCount' over a one-item friend list.

    /// it('limits stream batches to the default capacity (100)')
    ///
    /// A ready source fills the buffer to the default capacity in one
    /// generation: one turn of 100, then the last item with the completion.
    #[test]
    fn limits_stream_batches_to_the_default_capacity_100() {
        let world = World {
            friend_list: Source::List((0..101).map(|i| Ok(Some(friend(i % 3)))).collect()),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            r#"
      query {
        friendList @stream {
          id
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        let first_batch: Vec<Value> = (0..100)
            .map(|i| json!({"id": ((i % 3) + 1).to_string()}))
            .collect();
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"friendList": []},
                    "pending": [{"id": "0", "path": ["friendList"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": first_batch}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "items": [{"id": "2"}]}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }
}

/// describe('Execute: stream directive (cancellation)')
///
/// Every case drives an abort signal or the result iterator's `return()`
/// and asserts on rejections and `return` spies, with no payload sequence
/// of its own: greem cancels by dropping the response stream.
mod stream_directive_cancellation {
    // it('should stop streamed execution when aborted')
    // Not ported, reason (ii): abort signal.

    // it('cancels streaming when aborted during async iterator next')
    // Not ported, reason (ii): abort signal.

    // it('waits for async stream source return cleanup before abort cancellation settles')
    // Not ported, reason (ii): abort signal and the source's `return` timing.

    // it('waits for async deferred nested stream item cleanup before abort cancellation settles')
    // Not ported, reason (ii): abort signal and the source's `return` timing.

    // it('cancels streaming when aborted while item promise is pending')
    // Not ported, reason (ii): abort signal.

    // it('cancels pending stream item executors with deferred work when consumer cancels')
    // Not ported, reason (ii): `return()` on the result iterator and a resolver spy.

    // it('stops when the stream queue is back-pressured and the consumer cancels')
    // Not ported, reason (ii): `return()` on the result iterator and the source's `return` spy.

    // it('cancels tasks and streams when aborted before initial execution finishes')
    // Not ported, reason (ii): abort signal.

    // it('cancels async stream source cleanup when aborted before initial execution finishes')
    // Not ported, reason (ii): abort signal and the source's `return` spy.

    // it('should ignore repeated cancellation attempts during incremental execution')
    // Not ported, reason (ii): abort signal.

    // it('cancels stream item executors with deferred work and nested streams')
    // Not ported, reason (ii): `return()` on the result iterator; nothing is asserted on payloads.

    // it('stops streaming when a pending stream item resolves after cancellation')
    // Not ported, reason (ii): `return()` on the result iterator while an item is pending.
}
