//! graphql-js v17.0.2 `src/execution/__tests__/executor-test.ts`, case by case in upstream order.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::executor::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// [`Area::single`] of one operation of a multi-operation document.
fn single_named(world: World, query: &str, operation_name: &str) -> Value {
    world
        .single_operation(
            Some(operation_name),
            query,
            Value::Null,
            ExecuteOptions::default(),
        )
        .0
}

/// describe('Execute: Handles basic execution tasks')
mod handles_basic_execution_tasks {
    use super::*;
    use greem_compliance::graphql_js::executor::{DeepData, QueryRoot};

    /// it('executes arbitrary code')
    #[test]
    fn executes_arbitrary_code() {
        let data = World {
            a: Some("Apple".into()),
            b: Some("Banana".into()),
            c: Some("Cookie".into()),
            d: Some("Donut".into()),
            e: Some("Egg".into()),
            f: Some("Fish".into()),
            deep: Some(DeepData {
                a: Some("Already Been Done".into()),
                b: Some("Boring".into()),
                c: Some(vec![
                    Some("Contrived".into()),
                    None,
                    Some("Confusing".into()),
                ]),
                deeper: Some(vec![Some(QueryRoot), None, Some(QueryRoot)]),
            }),
            promise: Some(QueryRoot),
            ..Default::default()
        };
        let (v, _) = data.single(
            r#"
      query ($size: Int) {
        a,
        b,
        x: c
        ...c
        f
        ...on DataType {
          pic(size: $size)
          promise {
            a
          }
        }
        deep {
          a
          b
          c
          deeper {
            a
            b
          }
        }
      }

      fragment c on DataType {
        d
        e
      }
    "#,
            json!({"size": 100}),
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "a": "Apple",
                    "b": "Banana",
                    "x": "Cookie",
                    "d": "Donut",
                    "e": "Egg",
                    "f": "Fish",
                    "pic": "Pic of size: 100",
                    "promise": {"a": "Apple"},
                    "deep": {
                        "a": "Already Been Done",
                        "b": "Boring",
                        "c": ["Contrived", null, "Confusing"],
                        "deeper": [
                            {"a": "Apple", "b": "Banana"},
                            null,
                            {"a": "Apple", "b": "Banana"},
                        ],
                    },
                },
            }),
        );
    }

    /// it('merges parallel fragments')
    ///
    /// `Type.deep` is `deepType` here (the area's `deep` is `DataType`'s).
    /// Response keys follow field collection: `deepType` is first collected
    /// under `FragOne`, before `FragTwo`'s `c`.
    #[test]
    fn merges_parallel_fragments() {
        let (v, _) = World {
            a: Some("Apple".into()),
            b: Some("Banana".into()),
            c: Some("Cherry".into()),
            deep_type: Some(QueryRoot),
            ..Default::default()
        }
        .single(
            r#"
      { a, ...FragOne, ...FragTwo }

      fragment FragOne on Type {
        b
        deepType { b, deeper: deepType { b } }
      }

      fragment FragTwo on Type {
        c
        deepType { c, deeper: deepType { c } }
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "a": "Apple",
                    "b": "Banana",
                    "deepType": {
                        "b": "Banana",
                        "deeper": {
                            "b": "Banana",
                            "c": "Cherry",
                        },
                        "c": "Cherry",
                    },
                    "c": "Cherry",
                },
            }),
        );
    }

    // it('provides info about current execution state')
    // Not ported, reason (i): asserts the resolver's `info` contents.

    // it('populates path correctly with complex types')
    // Not ported, reason (i): asserts the resolver's `info.path`.

    // it('threads root value context correctly')
    // Not ported, reason (i): asserts `rootValue` identity inside the resolver.

    // it('correctly threads arguments')
    // Not ported, reason (i): asserts the arguments the resolver received, not the response.

    /// it('nulls out error subtrees')
    ///
    /// Resolver errors carry the resolver's own text: upstream wraps a thrown
    /// non-`Error` as `Unexpected error value: "..."`, and a value-less
    /// rejection as `Unexpected error value: undefined`, which is an empty
    /// message here.
    #[test]
    fn nulls_out_error_subtrees() {
        let (v, _) = World {
            async_error: Some("Error getting asyncError"),
            ..Default::default()
        }
        .single(
            r#"
      {
        sync
        syncError
        syncRawError
        syncReturnError
        syncReturnErrorList
        async
        asyncReject
        asyncRawReject
        asyncEmptyReject
        asyncError
        asyncRawError
        asyncReturnError
        asyncReturnErrorWithExtensions
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "sync": "sync",
                    "syncError": null,
                    "syncRawError": null,
                    "syncReturnError": null,
                    "syncReturnErrorList": ["sync0", null, "sync2", null],
                    "async": "async",
                    "asyncReject": null,
                    "asyncRawReject": null,
                    "asyncEmptyReject": null,
                    "asyncError": null,
                    "asyncRawError": null,
                    "asyncReturnError": null,
                    "asyncReturnErrorWithExtensions": null,
                },
                "errors": [
                    {
                        "message": "Error getting syncError",
                        "locations": [{"line": 4, "column": 9}],
                        "path": ["syncError"],
                    },
                    {
                        "message": "Error getting syncRawError",
                        "locations": [{"line": 5, "column": 9}],
                        "path": ["syncRawError"],
                    },
                    {
                        "message": "Error getting syncReturnError",
                        "locations": [{"line": 6, "column": 9}],
                        "path": ["syncReturnError"],
                    },
                    {
                        "message": "Error getting syncReturnErrorList1",
                        "locations": [{"line": 7, "column": 9}],
                        "path": ["syncReturnErrorList", 1],
                    },
                    {
                        "message": "Error getting syncReturnErrorList3",
                        "locations": [{"line": 7, "column": 9}],
                        "path": ["syncReturnErrorList", 3],
                    },
                    {
                        "message": "Error getting asyncReject",
                        "locations": [{"line": 9, "column": 9}],
                        "path": ["asyncReject"],
                    },
                    {
                        "message": "Error getting asyncRawReject",
                        "locations": [{"line": 10, "column": 9}],
                        "path": ["asyncRawReject"],
                    },
                    {
                        "message": "",
                        "locations": [{"line": 11, "column": 9}],
                        "path": ["asyncEmptyReject"],
                    },
                    {
                        "message": "Error getting asyncError",
                        "locations": [{"line": 12, "column": 9}],
                        "path": ["asyncError"],
                    },
                    {
                        "message": "Error getting asyncRawError",
                        "locations": [{"line": 13, "column": 9}],
                        "path": ["asyncRawError"],
                    },
                    {
                        "message": "Error getting asyncReturnError",
                        "locations": [{"line": 14, "column": 9}],
                        "path": ["asyncReturnError"],
                    },
                    {
                        "message": "Error getting asyncReturnErrorWithExtensions",
                        "locations": [{"line": 15, "column": 9}],
                        "path": ["asyncReturnErrorWithExtensions"],
                        "extensions": {"foo": "bar"},
                    },
                ],
            }),
        );
    }

    /// it('nulls error subtree for promise rejection #1071')
    #[test]
    fn nulls_error_subtree_for_promise_rejection_1071() {
        let (v, _) = World::default().single(
            r#"
      query {
        foods {
          name
        }
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {"foods": null},
                "errors": [{
                    "message": "Oops",
                    "locations": [{"line": 3, "column": 9}],
                    "path": ["foods"],
                }],
            }),
        );
    }

    // it('handles sync errors combined with rejections')
    // Not ported, reason (iv): both resolvers return null at a non-null position; the
    // single error asserts the sync throw pre-empting the still-pending promise (ii).

    /// it('handles async bubbling errors combined with non-bubbling errors')
    ///
    /// `asyncNonNullError` fails after one yield where upstream returns null;
    /// the non-null error propagates to `data` and the nullable one is kept.
    #[test]
    fn handles_async_bubbling_errors_combined_with_non_bubbling_errors() {
        let (v, _) = World {
            async_error: Some("Oops"),
            ..Default::default()
        }
        .single(
            r#"
      {
        asyncError
        asyncNonNullError
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
                        "message": "Oops",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["asyncError"],
                    },
                    {
                        "message": "Cannot return null for non-nullable field Query.asyncNonNullError.",
                        "locations": [{"line": 4, "column": 9}],
                        "path": ["asyncNonNullError"],
                    },
                ],
            }),
        );
    }

    /// it('Full response path is included for non-nullable fields')
    #[test]
    fn full_response_path_is_included_for_non_nullable_fields() {
        let (v, _) = World::default().single(
            r#"
      query {
        nullableA {
          aliasedA: nullableA {
            nonNullA {
              anotherA: nonNullA {
                throws
              }
            }
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
                "data": {
                    "nullableA": {
                        "aliasedA": null,
                    },
                },
                "errors": [{
                    "message": "Catch me if you can",
                    "locations": [{"line": 7, "column": 17}],
                    "path": ["nullableA", "aliasedA", "nonNullA", "anotherA", "throws"],
                }],
            }),
        );
    }

    /// it('uses the inline operation if no operation name is provided')
    #[test]
    fn uses_the_inline_operation_if_no_operation_name_is_provided() {
        let (v, _) = World {
            a: Some("b".into()),
            ..Default::default()
        }
        .single("{ a }", Value::Null, ExecuteOptions::default());
        assert_response(&v, &json!({"data": {"a": "b"}}));
    }

    /// it('uses the only operation if no operation name is provided')
    #[test]
    fn uses_the_only_operation_if_no_operation_name_is_provided() {
        let (v, _) = World {
            a: Some("b".into()),
            ..Default::default()
        }
        .single(
            "query Example { a }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(&v, &json!({"data": {"a": "b"}}));
    }

    /// it('uses the named operation if operation name is provided')
    #[test]
    fn uses_the_named_operation_if_operation_name_is_provided() {
        let v = single_named(
            World {
                a: Some("b".into()),
                ..Default::default()
            },
            r#"
      query Example { first: a }
      query OtherExample { second: a }
    "#,
            "OtherExample",
        );
        assert_response(&v, &json!({"data": {"second": "b"}}));
    }

    /// it('provides error if no operation is provided')
    ///
    /// A document of one unused fragment fails validation at parse; the
    /// wording is apollo-compiler's, learned by running it once.
    #[test]
    fn provides_error_if_no_operation_is_provided() {
        let (v, _) = World {
            a: Some("b".into()),
            ..Default::default()
        }
        .single(
            "fragment Example on Type { a }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "fragment `Example` must be used in an operation",
                    "locations": [{"line": 1, "column": 1}],
                }],
            }),
        );
    }

    /// it('errors if no op name is provided with multiple operations')
    ///
    /// The request error's wording is apollo-compiler's, learned by running it once.
    #[test]
    fn errors_if_no_op_name_is_provided_with_multiple_operations() {
        let (v, _) = World::default().single(
            r#"
      query Example { a }
      query OtherExample { a }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "Ambiguous request: multiple operations but no specified `operationName`",
                }],
            }),
        );
    }

    /// it('errors if unknown operation name is provided')
    ///
    /// The request error's wording is apollo-compiler's, learned by running it once.
    #[test]
    fn errors_if_unknown_operation_name_is_provided() {
        let v = single_named(
            World::default(),
            r#"
      query Example { a }
      query OtherExample { a }
    "#,
            "UnknownExample",
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "No operation named 'UnknownExample'",
                }],
            }),
        );
    }

    /// it('errors if empty string is provided as operation name')
    ///
    /// The request error's wording is apollo-compiler's, learned by running it once.
    #[test]
    fn errors_if_empty_string_is_provided_as_operation_name() {
        let v = single_named(World::default(), "{ a }", "");
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "No operation named ''",
                }],
            }),
        );
    }

    /// it('uses the query schema for queries')
    #[test]
    fn uses_the_query_schema_for_queries() {
        let v = single_named(
            World {
                a: Some("b".into()),
                c: Some("d".into()),
                ..Default::default()
            },
            r#"
      query Q { a }
      mutation M { c }
      subscription S { a }
    "#,
            "Q",
        );
        assert_response(&v, &json!({"data": {"a": "b"}}));
    }

    /// it('uses the mutation schema for mutations')
    #[test]
    fn uses_the_mutation_schema_for_mutations() {
        let v = single_named(
            World {
                a: Some("b".into()),
                c: Some("d".into()),
                ..Default::default()
            },
            r#"
      query Q { a }
      mutation M { c }
    "#,
            "M",
        );
        assert_response(&v, &json!({"data": {"c": "d"}}));
    }

    /// it('uses the subscription schema for subscriptions')
    ///
    /// greem has no subscriptions yet: the request is rejected at execute
    /// with "subscriptions are not supported".
    #[test]
    #[ignore = "#40"]
    fn uses_the_subscription_schema_for_subscriptions() {
        let v = single_named(
            World {
                a: Some("b".into()),
                c: Some("d".into()),
                ..Default::default()
            },
            r#"
      query Q { a }
      subscription S { a }
    "#,
            "S",
        );
        assert_response(&v, &json!({"data": {"a": "b"}}));
    }

    // it('resolves to an error if schema does not support operation')
    // Not ported, reason (iv): a greem schema always has a `Query`, and this area declares
    // all three roots, so no operation type can be unsupported here.

    /// it('correct field ordering despite execution order')
    #[test]
    fn correct_field_ordering_despite_execution_order() {
        let (v, _) = World {
            a: Some("a".into()),
            b: Some("b".into()),
            c: Some("c".into()),
            d: Some("d".into()),
            e: Some("e".into()),
            promised: &["b", "d"],
            ..Default::default()
        }
        .single("{ a, b, c, d, e }", Value::Null, ExecuteOptions::default());
        assert_response(
            &v,
            &json!({
                "data": {"a": "a", "b": "b", "c": "c", "d": "d", "e": "e"},
            }),
        );
    }

    /// it('Avoids recursion')
    ///
    /// Upstream executes without validating and gets `data: {a: 'b'}`; greem
    /// validates at parse, so the fragment cycle is a request error. The
    /// wording is apollo-compiler's, learned by running it once.
    #[test]
    fn avoids_recursion() {
        let (v, _) = World {
            a: Some("b".into()),
            ..Default::default()
        }
        .single(
            r#"
      {
        a
        ...Frag
        ...Frag
      }

      fragment Frag on Type {
        a,
        ...Frag
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "`Frag` fragment cannot reference itself",
                    "locations": [{"line": 8, "column": 7}],
                }],
            }),
        );
    }

    /// it('ignores missing sub selections on fields')
    ///
    /// Upstream's `a: SomeType` is `aObject` here. Upstream executes without
    /// validating and gets `data: {a: {}}`; greem validates at parse, so the
    /// missing selection set is a request error. The wording is
    /// apollo-compiler's, learned by running it once.
    #[test]
    fn ignores_missing_sub_selections_on_fields() {
        let (v, _) = World::default().single("{ aObject }", Value::Null, ExecuteOptions::default());
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "interface, union and object types must have a subselection set",
                    "locations": [{"line": 1, "column": 3}],
                }],
            }),
        );
    }

    /// it('does not include illegal fields in output')
    ///
    /// Upstream executes without validating and gets `data: {}`; greem
    /// validates at parse, so the unknown field is a request error.
    #[test]
    fn does_not_include_illegal_fields_in_output() {
        let (v, _) = World::default().single(
            "{ thisIsIllegalDoNotIncludeMe }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "type `Query` does not have a field `thisIsIllegalDoNotIncludeMe`",
                    "locations": [{"line": 1, "column": 3}],
                }],
            }),
        );
    }

    /// it('does not include arguments that were not set')
    #[test]
    fn does_not_include_arguments_that_were_not_set() {
        let (v, _) = World::default().single(
            "{ field(a: true, c: false, e: 0) }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "field": "{ a: true, c: false, e: 0 }",
                },
            }),
        );
    }

    // it('fails when an isTypeOf check is not met')
    // Not ported, reason (iv): a value of another Rust type at a `SpecialType` position
    // does not compile.

    // it('fails when coerceOutputValue of custom scalar does not return a value')
    // Not ported, reason (iv): a scalar codec returns a `Value`; there is no "returned
    // nothing".

    /// it('executes ignoring invalid non-executable definitions')
    ///
    /// Upstream ignores the type definition; greem's executable parse rejects
    /// it as a request error. The wording is apollo-compiler's, learned by
    /// running it once.
    #[test]
    fn executes_ignoring_invalid_non_executable_definitions() {
        let (v, _) = World::default().single(
            r#"
      { foo }

      type Query { bar: String }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "an executable document must not contain an object type definition",
                    "locations": [{"line": 4, "column": 7}],
                }],
            }),
        );
    }

    // it('uses a custom field resolver')
    // Not ported, reason (iii): the `fieldResolver` option.

    // it('uses a custom type resolver')
    // Not ported, reason (iii): the `typeResolver` option.

    // it('uses a different number of max coercion errors')
    // Not ported, reason (iii): the `maxCoercionErrors` option.

    // it('memoizes collectSubfields results')
    // Not ported, reason (ii): identity of `collectSubfields` results, an internal API.

    // it('memoizes getStreamUsage results')
    // Not ported, reason (ii): identity of `getStreamUsage` results, an internal API.
}
