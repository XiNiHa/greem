//! graphql-js v17.0.2 `src/execution/__tests__/nonnull-test.ts`, case by
//! case in upstream order.
//!
//! Upstream runs the `nulls ...` cases through `executeSyncAndAsync`, which
//! repeats the query with `sync` rewritten to `promise`; the sync-ness is not
//! the claim, so each ports once. `errors` are listed in greem's order
//! (generation, then tree order, then field) where upstream lists them in
//! completion order.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::nonnull::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// describe('Execute: handles non-nullable types')
mod handles_non_nullable_types {
    use super::*;

    /// describe('nulls a nullable field')
    mod nulls_a_nullable_field {
        use super::*;

        const QUERY: &str = r#"
      {
        sync
      }
    "#;

        /// it('that returns null')
        #[test]
        fn that_returns_null() {
            let (v, _) = World::nulling().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(&v, &json!({"data": {"sync": null}}));
        }

        /// it('that throws')
        #[test]
        fn that_throws() {
            let (v, _) = World::throwing().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(
                &v,
                &json!({
                    "data": {"sync": null},
                    "errors": [{
                        "message": "sync",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["sync"],
                    }],
                }),
            );
        }
    }

    /// describe('nulls a returned object that contains a non-nullable field')
    mod nulls_a_returned_object_that_contains_a_non_nullable_field {
        use super::*;

        const QUERY: &str = r#"
      {
        syncNest {
          syncNonNull,
        }
      }
    "#;

        // it('that returns null')
        // Not ported, reason (iv): `syncNonNull` returning null at a non-null position.

        /// it('that throws')
        #[test]
        fn that_throws() {
            let (v, _) = World::throwing().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(
                &v,
                &json!({
                    "data": {"syncNest": null},
                    "errors": [{
                        "message": "syncNonNull",
                        "locations": [{"line": 4, "column": 11}],
                        "path": ["syncNest", "syncNonNull"],
                    }],
                }),
            );
        }
    }

    /// describe('nulls a complex tree of nullable fields, each')
    mod nulls_a_complex_tree_of_nullable_fields_each {
        use super::*;

        const QUERY: &str = r#"
      {
        syncNest {
          sync
          promise
          syncNest { sync promise }
          promiseNest { sync promise }
        }
        promiseNest {
          sync
          promise
          syncNest { sync promise }
          promiseNest { sync promise }
        }
      }
    "#;

        fn data() -> Value {
            json!({
                "syncNest": {
                    "sync": null,
                    "promise": null,
                    "syncNest": {"sync": null, "promise": null},
                    "promiseNest": {"sync": null, "promise": null},
                },
                "promiseNest": {
                    "sync": null,
                    "promise": null,
                    "syncNest": {"sync": null, "promise": null},
                    "promiseNest": {"sync": null, "promise": null},
                },
            })
        }

        /// it('that returns null')
        #[test]
        fn that_returns_null() {
            let (v, _) = World::nulling().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(&v, &json!({"data": data()}));
        }

        /// it('that throws')
        ///
        /// Every leaf errors and every leaf is nullable, so nothing
        /// propagates. The errors come generation by generation: the four
        /// direct leaves of the two root nests, then the eight beneath them.
        #[test]
        fn that_throws() {
            let (v, _) = World::throwing().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(
                &v,
                &json!({
                    "data": data(),
                    "errors": [
                        {
                            "message": "sync",
                            "locations": [{"line": 4, "column": 11}],
                            "path": ["syncNest", "sync"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 5, "column": 11}],
                            "path": ["syncNest", "promise"],
                        },
                        {
                            "message": "sync",
                            "locations": [{"line": 10, "column": 11}],
                            "path": ["promiseNest", "sync"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 11, "column": 11}],
                            "path": ["promiseNest", "promise"],
                        },
                        {
                            "message": "sync",
                            "locations": [{"line": 6, "column": 22}],
                            "path": ["syncNest", "syncNest", "sync"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 6, "column": 27}],
                            "path": ["syncNest", "syncNest", "promise"],
                        },
                        {
                            "message": "sync",
                            "locations": [{"line": 7, "column": 25}],
                            "path": ["syncNest", "promiseNest", "sync"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 7, "column": 30}],
                            "path": ["syncNest", "promiseNest", "promise"],
                        },
                        {
                            "message": "sync",
                            "locations": [{"line": 12, "column": 22}],
                            "path": ["promiseNest", "syncNest", "sync"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 12, "column": 27}],
                            "path": ["promiseNest", "syncNest", "promise"],
                        },
                        {
                            "message": "sync",
                            "locations": [{"line": 13, "column": 25}],
                            "path": ["promiseNest", "promiseNest", "sync"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 13, "column": 30}],
                            "path": ["promiseNest", "promiseNest", "promise"],
                        },
                    ],
                }),
            );
        }
    }

    /// describe('nulls the first nullable object after a field in a long chain of non-null fields')
    mod nulls_the_first_nullable_object_after_a_field_in_a_long_chain_of_non_null_fields {
        use super::*;

        const QUERY: &str = r#"
      {
        syncNest {
          syncNonNullNest {
            promiseNonNullNest {
              syncNonNullNest {
                promiseNonNullNest {
                  syncNonNull
                }
              }
            }
          }
        }
        promiseNest {
          syncNonNullNest {
            promiseNonNullNest {
              syncNonNullNest {
                promiseNonNullNest {
                  syncNonNull
                }
              }
            }
          }
        }
        anotherNest: syncNest {
          syncNonNullNest {
            promiseNonNullNest {
              syncNonNullNest {
                promiseNonNullNest {
                  promiseNonNull
                }
              }
            }
          }
        }
        anotherPromiseNest: promiseNest {
          syncNonNullNest {
            promiseNonNullNest {
              syncNonNullNest {
                promiseNonNullNest {
                  promiseNonNull
                }
              }
            }
          }
        }
      }
    "#;

        // it('that returns null')
        // Not ported, reason (iv): `syncNonNull` and `promiseNonNull` returning null at a non-null position.

        /// it('that throws')
        ///
        /// Each leaf error climbs four non-null nests to the nullable root
        /// field, which is the first position that can absorb it.
        #[test]
        fn that_throws() {
            let (v, _) = World::throwing().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(
                &v,
                &json!({
                    "data": {
                        "syncNest": null,
                        "promiseNest": null,
                        "anotherNest": null,
                        "anotherPromiseNest": null,
                    },
                    "errors": [
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 8, "column": 19}],
                            "path": [
                                "syncNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "syncNonNull",
                            ],
                        },
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 19, "column": 19}],
                            "path": [
                                "promiseNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "syncNonNull",
                            ],
                        },
                        {
                            "message": "promiseNonNull",
                            "locations": [{"line": 30, "column": 19}],
                            "path": [
                                "anotherNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "promiseNonNull",
                            ],
                        },
                        {
                            "message": "promiseNonNull",
                            "locations": [{"line": 41, "column": 19}],
                            "path": [
                                "anotherPromiseNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "syncNonNullNest",
                                "promiseNonNullNest",
                                "promiseNonNull",
                            ],
                        },
                    ],
                }),
            );
        }
    }

    /// describe('nulls the top level if non-nullable field')
    mod nulls_the_top_level_if_non_nullable_field {
        use super::*;

        const QUERY: &str = r#"
      {
        syncNonNull
      }
    "#;

        // it('that returns null')
        // Not ported, reason (iv): `syncNonNull` returning null at a non-null position.

        /// it('that throws')
        #[test]
        fn that_throws() {
            let (v, _) = World::throwing().single(QUERY, Value::Null, ExecuteOptions::default());
            assert_response(
                &v,
                &json!({
                    "data": null,
                    "errors": [{
                        "message": "syncNonNull",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["syncNonNull"],
                    }],
                }),
            );
        }
    }

    /// describe('Handles multiple errors for a single response position')
    ///
    /// Upstream's "slower" cases drop the error that lands after its parent
    /// was already nulled: a timing artefact of bubbling during execution.
    /// greem resolves every field of a generation whatever the interleaving
    /// and lists every error raised (spec 6.4.4: a field error "must be added
    /// to the errors list"), so those cases list both errors here.
    mod handles_multiple_errors_for_a_single_response_position {
        use super::*;

        /// it('nullable and non-nullable root fields throw nested errors')
        #[test]
        fn nullable_and_non_nullable_root_fields_throw_nested_errors() {
            let (v, _) = World::throwing().single(
                r#"
        {
          promiseNonNullNest {
            syncNonNull
          }
          promiseNest {
            syncNonNull
          }
        }
      "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": null,
                    "errors": [
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 4, "column": 13}],
                            "path": ["promiseNonNullNest", "syncNonNull"],
                        },
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 7, "column": 13}],
                            "path": ["promiseNest", "syncNonNull"],
                        },
                    ],
                }),
            );
        }

        /// it('a nullable root field throws a slower nested error after a non-nullable root field throws a nested error')
        #[test]
        fn a_nullable_root_field_throws_a_slower_nested_error_after_a_non_nullable_root_field_throws_a_nested_error()
         {
            let (v, _) = World::throwing().single(
                r#"
        {
          promiseNonNullNest {
            syncNonNull
          }
          promiseNest {
            promiseNonNull
          }
        }
      "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            // upstream: only the `promiseNonNullNest.syncNonNull` error; see the module doc.
            assert_response(
                &v,
                &json!({
                    "data": null,
                    "errors": [
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 4, "column": 13}],
                            "path": ["promiseNonNullNest", "syncNonNull"],
                        },
                        {
                            "message": "promiseNonNull",
                            "locations": [{"line": 7, "column": 13}],
                            "path": ["promiseNest", "promiseNonNull"],
                        },
                    ],
                }),
            );
        }

        /// it('nullable and non-nullable nested fields throw nested errors')
        #[test]
        fn nullable_and_non_nullable_nested_fields_throw_nested_errors() {
            let (v, _) = World::throwing().single(
                r#"
        {
          syncNest {
            promiseNonNullNest {
              syncNonNull
            }
            promiseNest {
              syncNonNull
            }
          }
        }
      "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {"syncNest": null},
                    "errors": [
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 5, "column": 15}],
                            "path": ["syncNest", "promiseNonNullNest", "syncNonNull"],
                        },
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 8, "column": 15}],
                            "path": ["syncNest", "promiseNest", "syncNonNull"],
                        },
                    ],
                }),
            );
        }

        /// it('a nullable nested field throws a slower nested error after a non-nullable nested field throws a nested error')
        #[test]
        fn a_nullable_nested_field_throws_a_slower_nested_error_after_a_non_nullable_nested_field_throws_a_nested_error()
         {
            let (v, _) = World::throwing().single(
                r#"
        {
          syncNest {
            promiseNonNullNest {
              syncNonNull
            }
            promiseNest {
              promiseNest {
                promiseNest {
                  promiseNonNull
                }
              }
            }
          }
        }
      "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            // upstream: only the `syncNest.promiseNonNullNest.syncNonNull` error; see the module doc.
            assert_response(
                &v,
                &json!({
                    "data": {"syncNest": null},
                    "errors": [
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 5, "column": 15}],
                            "path": ["syncNest", "promiseNonNullNest", "syncNonNull"],
                        },
                        {
                            "message": "promiseNonNull",
                            "locations": [{"line": 10, "column": 19}],
                            "path": [
                                "syncNest",
                                "promiseNest",
                                "promiseNest",
                                "promiseNest",
                                "promiseNonNull",
                            ],
                        },
                    ],
                }),
            );
        }

        /// it('suppresses a later error after a parent has been nulled')
        ///
        /// Upstream's root value rejects `syncNonNull` first and `promise`
        /// three ticks later; here `promise` takes three extra yields.
        #[test]
        fn suppresses_a_later_error_after_a_parent_has_been_nulled() {
            let world = World {
                promise_delay: 3,
                ..World::throwing()
            };
            let (v, _) = world.single(
                r#"
        {
          syncNest {
            syncNonNull
            promise
          }
        }
      "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            // upstream: only the `syncNest.syncNonNull` error; see the module doc.
            assert_response(
                &v,
                &json!({
                    "data": {"syncNest": null},
                    "errors": [
                        {
                            "message": "syncNonNull",
                            "locations": [{"line": 4, "column": 13}],
                            "path": ["syncNest", "syncNonNull"],
                        },
                        {
                            "message": "promise",
                            "locations": [{"line": 5, "column": 13}],
                            "path": ["syncNest", "promise"],
                        },
                    ],
                }),
            );
        }
    }

    /// describe('Handles non-null argument')
    ///
    /// Upstream's `schemaWithNonNullArg` is `DataType.withNonNullArg` here.
    /// Upstream executes the invalid documents without validating them and
    /// gets execution errors; greem validates at parse, so those are request
    /// errors in apollo-compiler's wording (learned by running greem).
    mod handles_non_null_argument {
        use super::*;

        /// it('succeeds when passed non-null literal value')
        #[test]
        fn succeeds_when_passed_non_null_literal_value() {
            let (v, _) = World::throwing().single(
                r#"
          query {
            withNonNullArg (cannotBeNull: "literal value")
          }
        "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({"data": {"withNonNullArg": "Passed: literal value"}}),
            );
        }

        /// it('succeeds when passed non-null variable value')
        #[test]
        fn succeeds_when_passed_non_null_variable_value() {
            let (v, _) = World::throwing().single(
                r#"
          query ($testVar: String!) {
            withNonNullArg (cannotBeNull: $testVar)
          }
        "#,
                json!({"testVar": "variable value"}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({"data": {"withNonNullArg": "Passed: variable value"}}),
            );
        }

        /// it('succeeds when missing variable has default value')
        #[test]
        fn succeeds_when_missing_variable_has_default_value() {
            let (v, _) = World::throwing().single(
                r#"
          query ($testVar: String = "default value") {
            withNonNullArg (cannotBeNull: $testVar)
          }
        "#,
                json!({}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({"data": {"withNonNullArg": "Passed: default value"}}),
            );
        }

        /// it('field error when missing non-null arg')
        #[test]
        fn field_error_when_missing_non_null_arg() {
            let (v, _) = World::throwing().single(
                r#"
          query {
            withNonNullArg
          }
        "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "the required argument `DataType.withNonNullArg(cannotBeNull:)` is not provided",
                        "locations": [{"line": 3, "column": 13}],
                    }],
                }),
            );
        }

        /// it('field error when non-null arg provided null')
        ///
        /// apollo-compiler counts a null literal as not providing the
        /// required argument and reports the literal's type too.
        #[test]
        fn field_error_when_non_null_arg_provided_null() {
            let (v, _) = World::throwing().single(
                r#"
          query {
            withNonNullArg(cannotBeNull: null)
          }
        "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [
                        {
                            "message": "the required argument `DataType.withNonNullArg(cannotBeNull:)` is not provided",
                            "locations": [{"line": 3, "column": 13}],
                        },
                        {
                            "message": "expected value of type String!, found null",
                            "locations": [{"line": 3, "column": 42}],
                        },
                    ],
                }),
            );
        }

        /// it('field error when non-null arg not provided variable value')
        #[test]
        fn field_error_when_non_null_arg_not_provided_variable_value() {
            let (v, _) = World::throwing().single(
                r#"
          query ($testVar: String) {
            withNonNullArg(cannotBeNull: $testVar)
          }
        "#,
                json!({}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "variable `$testVar` of type `String` cannot be used for argument `cannotBeNull` of type `String!`",
                        "locations": [{"line": 3, "column": 28}],
                    }],
                }),
            );
        }

        /// it('field error when non-null arg provided variable with explicit null value')
        ///
        /// Valid: a variable with a default may sit at a non-null position.
        /// The explicit null survives variable coercion, so argument
        /// coercion raises a execution error at the field (spec 6.4.1), located
        /// at the field rather than upstream's argument.
        #[test]
        fn field_error_when_non_null_arg_provided_variable_with_explicit_null_value() {
            let (v, _) = World::throwing().single(
                r#"
          query ($testVar: String = "default value") {
            withNonNullArg (cannotBeNull: $testVar)
          }
        "#,
                json!({"testVar": null}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {"withNonNullArg": null},
                    "errors": [{
                        "message": "argument `cannotBeNull` of non-null type `String!` must not be null",
                        "locations": [{"line": 3, "column": 13}],
                        "path": ["withNonNullArg"],
                        "extensions": {"code": "BAD_USER_INPUT"},
                    }],
                }),
            );
        }
    }
}
