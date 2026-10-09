//! graphql-js v17.0.2 `src/execution/__tests__/variables-test.ts`, case by case in upstream order.
//!
//! Resolvers echo the coerced input as JSON where upstream uses `inspect`,
//! so `{ a: "foo" }` reads `{"a":"foo"}`, and an input object lists every
//! field in schema order with an absent one as null (greem's `Option`
//! folds absent into null unless the type is `absent_aware`).
//!
//! Variable coercion is apollo-compiler's and a failure is a request error,
//! where upstream reports the same failure with its own wording; literal
//! arguments it cannot type-check (custom scalars) fail at greem's argument
//! coercion as a execution error with `extensions.code: "BAD_USER_INPUT"`.
//! The apollo-compiler messages and locations below were learnt by running
//! greem once per distinct wording; every other value is derived by hand.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::variables::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

fn execute_query(query: &str, variables: Value) -> Value {
    World::default()
        .single(query, variables, ExecuteOptions::default())
        .0
}

/// Upstream's `TestInputObject` echo, with its absent fields as null.
const COMPLEX_INPUT: &str = r#"{"a":"foo","b":["bar"],"c":"baz","d":null,"e":null}"#;

/// describe('Execute: Handles inputs')
mod handles_inputs {
    use super::*;

    /// describe('Handles objects and nullability')
    mod handles_objects_and_nullability {
        use super::*;

        /// describe('using inline structs')
        mod using_inline_structs {
            use super::*;

            /// it('executes with complex input')
            #[test]
            fn executes_with_complex_input() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: {a: "foo", b: ["bar"], c: "baz"})
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithObjectInput": COMPLEX_INPUT}}),
                );
            }

            /// it('properly parses single value to list')
            #[test]
            fn properly_parses_single_value_to_list() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: {a: "foo", b: "bar", c: "baz"})
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithObjectInput": COMPLEX_INPUT}}),
                );
            }

            /// it('properly parses null value to null')
            #[test]
            fn properly_parses_null_value_to_null() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: {a: null, b: null, c: "C", d: null})
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({"data": {
                        "fieldWithObjectInput": r#"{"a":null,"b":null,"c":"C","d":null,"e":null}"#,
                    }}),
                );
            }

            /// it('properly parses null value in list')
            #[test]
            fn properly_parses_null_value_in_list() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: {b: ["A",null,"C"], c: "C"})
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({"data": {
                        "fieldWithObjectInput": r#"{"a":null,"b":["A",null,"C"],"c":"C","d":null,"e":null}"#,
                    }}),
                );
            }

            /// it('does not use incorrect value')
            ///
            /// Upstream executes without validating and gets a execution error;
            /// greem validates the literal at parse, so it is a request error.
            #[test]
            fn does_not_use_incorrect_value() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: ["foo", "bar", "baz"])
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "expected value of type TestInputObject, found a list",
                            "locations": [{"line": 3, "column": 41}],
                        }],
                    }),
                );
            }

            /// it('properly runs coerceInputLiteral on complex scalar types')
            #[test]
            fn properly_runs_coerce_input_literal_on_complex_scalar_types() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: {c: "foo", d: "ExternalValue"})
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({"data": {
                        "fieldWithObjectInput": r#"{"a":null,"b":null,"c":"foo","d":"InternalValue","e":null}"#,
                    }}),
                );
            }

            /// it('errors on faulty scalar type input')
            ///
            /// The scalar's `InputError` surfaces through the argument path
            /// (`input.e`) as greem's execution error.
            #[test]
            fn errors_on_faulty_scalar_type_input() {
                let v = execute_query(
                    r#"
          {
            fieldWithObjectInput(input: {c: "foo", e: "bar"})
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({
                        "data": {"fieldWithObjectInput": null},
                        "errors": [{
                            "message": "input.e: FaultyScalarErrorMessage",
                            "locations": [{"line": 3, "column": 13}],
                            "path": ["fieldWithObjectInput"],
                            "extensions": {"code": "BAD_USER_INPUT"},
                        }],
                    }),
                );
            }
        }

        /// describe('using variables')
        mod using_variables {
            use super::*;

            const DOC: &str = r#"
        query ($input: TestInputObject) {
          fieldWithObjectInput(input: $input)
        }
      "#;

            /// it('executes with complex input')
            #[test]
            fn executes_with_complex_input() {
                let v = execute_query(
                    DOC,
                    json!({"input": {"a": "foo", "b": ["bar"], "c": "baz"}}),
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithObjectInput": COMPLEX_INPUT}}),
                );
            }

            /// it('uses undefined when variable not provided')
            ///
            /// Spec 6.4.1: an argument whose variable is not provided has no
            /// entry, and upstream's resolver returns nothing for it. greem's
            /// `Option` argument reads `None` for absent and null alike, so
            /// the echo is `"null"`.
            #[test]
            #[ignore = "#38"]
            fn uses_undefined_when_variable_not_provided() {
                let v = execute_query(
                    r#"
          query q($input: String) {
            fieldWithNullableStringInput(input: $input)
          }"#,
                    json!({}),
                );
                assert_response(&v, &json!({"data": {"fieldWithNullableStringInput": null}}));
            }

            /// it('uses null when variable provided explicit null value')
            #[test]
            fn uses_null_when_variable_provided_explicit_null_value() {
                let v = execute_query(
                    r#"
          query q($input: String) {
            fieldWithNullableStringInput(input: $input)
          }"#,
                    json!({"input": null}),
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithNullableStringInput": "null"}}),
                );
            }

            /// it('preserves explicit null variables within input object literals')
            #[test]
            fn preserves_explicit_null_variables_within_input_object_literals() {
                let v = execute_query(
                    r#"
          query q($input: String) {
            fieldWithObjectInput(input: { a: $input, c: "baz" })
          }"#,
                    json!({"input": null}),
                );
                assert_response(
                    &v,
                    &json!({"data": {
                        "fieldWithObjectInput": r#"{"a":null,"b":null,"c":"baz","d":null,"e":null}"#,
                    }}),
                );
            }

            // it('treats explicitly undefined variable values as omitted')
            // Not ported, reason (ii): JSON variables cannot carry an explicit `undefined`.

            /// it('uses default value when not provided')
            #[test]
            fn uses_default_value_when_not_provided() {
                let v = execute_query(
                    r#"
          query ($input: TestInputObject = {a: "foo", b: ["bar"], c: "baz"}) {
            fieldWithObjectInput(input: $input)
          }
        "#,
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithObjectInput": COMPLEX_INPUT}}),
                );
            }

            /// it('reports invalid default values with variable definition locations')
            #[test]
            fn reports_invalid_default_values_with_variable_definition_locations() {
                let v = execute_query(
                    "query ($input: String = 123) { fieldWithNullableStringInput(input: $input) }",
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "expected value of type String, found an integer",
                            "locations": [{"line": 1, "column": 25}],
                        }],
                    }),
                );
            }

            /// it('includes suggestions for invalid default values')
            #[test]
            fn includes_suggestions_for_invalid_default_values() {
                let v = execute_query(
                    "query ($input: TestInputObject = { c: \"ok\", aa: \"x\" }) { fieldWithObjectInput(input: $input) }",
                    Value::Null,
                );
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "field `aa` does not exist on `TestInputObject`",
                            "locations": [{"line": 1, "column": 49}],
                        }],
                    }),
                );
            }

            // it('hides suggestions for invalid default values when specified')
            // Not ported, reason (iii): `hideSuggestions` is a graphql-js execution option.

            /// it('does not use default value when provided')
            #[test]
            fn does_not_use_default_value_when_provided() {
                let v = execute_query(
                    r#"
            query q($input: String = "Default value") {
              fieldWithNullableStringInput(input: $input)
            }
          "#,
                    json!({"input": "Variable value"}),
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithNullableStringInput": "\"Variable value\""}}),
                );
            }

            /// it('uses explicit null value instead of default value')
            #[test]
            fn uses_explicit_null_value_instead_of_default_value() {
                let v = execute_query(
                    r#"
          query q($input: String = "Default value") {
            fieldWithNullableStringInput(input: $input)
          }"#,
                    json!({"input": null}),
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithNullableStringInput": "null"}}),
                );
            }

            // it('treats explicitly undefined variable values as omitted')
            // Not ported, reason (ii): JSON variables cannot carry an explicit `undefined`.

            /// it('uses null default value when not provided')
            #[test]
            fn uses_null_default_value_when_not_provided() {
                let v = execute_query(
                    r#"
          query q($input: String = null) {
            fieldWithNullableStringInput(input: $input)
          }"#,
                    json!({}),
                );
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithNullableStringInput": "null"}}),
                );
            }

            /// it('properly parses single value to list')
            #[test]
            fn properly_parses_single_value_to_list() {
                let v = execute_query(DOC, json!({"input": {"a": "foo", "b": "bar", "c": "baz"}}));
                assert_response(
                    &v,
                    &json!({"data": {"fieldWithObjectInput": COMPLEX_INPUT}}),
                );
            }

            /// it('executes with complex scalar input')
            #[test]
            fn executes_with_complex_scalar_input() {
                let v = execute_query(DOC, json!({"input": {"c": "foo", "d": "ExternalValue"}}));
                assert_response(
                    &v,
                    &json!({"data": {
                        "fieldWithObjectInput": r#"{"a":null,"b":null,"c":"foo","d":"InternalValue","e":null}"#,
                    }}),
                );
            }

            /// it('errors on faulty scalar type input')
            ///
            /// greem reads a variable's custom scalars through their `Scalar`
            /// impls at variable coercion, so the scalar's `InputError`
            /// (at `e`) is a request error at the variable definition.
            #[test]
            fn errors_on_faulty_scalar_type_input() {
                let v = execute_query(DOC, json!({"input": {"c": "foo", "e": "ExternalValue"}}));
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "variable `$input` got an invalid value: e: FaultyScalarErrorMessage",
                            "locations": [{"line": 2, "column": 16}],
                        }],
                    }),
                );
            }

            /// it('errors on null for nested non-null')
            #[test]
            fn errors_on_null_for_nested_non_null() {
                let v = execute_query(DOC, json!({"input": {"a": "foo", "b": "bar", "c": null}}));
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "null value for input field TestInputObject.c of non-null type String!",
                        }],
                    }),
                );
            }

            /// it('errors on incorrect type')
            #[test]
            fn errors_on_incorrect_type() {
                let v = execute_query(DOC, json!({"input": "foo bar"}));
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "could not coerce variable input: \"foo bar\" to type TestInputObject",
                        }],
                    }),
                );
            }

            /// it('errors on omission of nested non-null')
            #[test]
            fn errors_on_omission_of_nested_non_null() {
                let v = execute_query(DOC, json!({"input": {"a": "foo", "b": "bar"}}));
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "Missing value for non-null input object field TestInputObject.c",
                        }],
                    }),
                );
            }

            /// it('errors on deep nested errors and with many errors')
            ///
            /// Upstream selects `fieldWithNestedObjectInput`, a field its
            /// schema does not have (the variable errors come first, so it
            /// never notices); greem validates, so the selection names the
            /// schema's `fieldWithNestedInputObject`. apollo-compiler stops
            /// at the first coercion failure, so one error where upstream
            /// lists two.
            #[test]
            fn errors_on_deep_nested_errors_and_with_many_errors() {
                let v = execute_query(
                    r#"
          query ($input: TestNestedInputObject) {
            fieldWithNestedInputObject(input: $input)
          }
        "#,
                    json!({"input": {"na": {"a": "foo"}}}),
                );
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "Missing value for non-null input object field TestInputObject.c",
                        }],
                    }),
                );
            }

            /// it('errors on addition of unknown input field')
            #[test]
            fn errors_on_addition_of_unknown_input_field() {
                let v = execute_query(
                    DOC,
                    json!({"input": {"a": "foo", "b": "bar", "c": "baz", "extra": "dog"}}),
                );
                assert_response(
                    &v,
                    &json!({
                        "errors": [{
                            "message": "Input object has key extra not in type TestInputObject",
                        }],
                    }),
                );
            }
        }
    }

    /// describe('Handles custom enum values')
    ///
    /// greem enums carry no internal value, so each echoes its name.
    mod handles_custom_enum_values {
        use super::*;

        /// it('allows custom enum values as inputs')
        #[test]
        fn allows_custom_enum_values_as_inputs() {
            let v = execute_query(
                r#"
        {
          null: fieldWithEnumInput(input: NULL)
          NaN: fieldWithEnumInput(input: NAN)
          false: fieldWithEnumInput(input: FALSE)
          customValue: fieldWithEnumInput(input: CUSTOM)
          defaultValue: fieldWithEnumInput(input: DEFAULT_VALUE)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {
                    "null": "\"NULL\"",
                    "NaN": "\"NAN\"",
                    "false": "\"FALSE\"",
                    "customValue": "\"CUSTOM\"",
                    "defaultValue": "\"DEFAULT_VALUE\"",
                }}),
            );
        }

        /// it('allows non-nullable inputs to have null as enum custom value')
        #[test]
        fn allows_non_nullable_inputs_to_have_null_as_enum_custom_value() {
            let v = execute_query(
                r#"
        {
          fieldWithNonNullableEnumInput(input: NULL)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNonNullableEnumInput": "\"NULL\""}}),
            );
        }
    }

    /// describe('Handles nullable scalars')
    mod handles_nullable_scalars {
        use super::*;

        /// it('allows nullable inputs to be omitted')
        ///
        /// Spec 6.4.1: an omitted argument has no entry; greem's `Option`
        /// reads `None` for absent and null alike, so the echo is `"null"`.
        #[test]
        #[ignore = "#38"]
        fn allows_nullable_inputs_to_be_omitted() {
            let v = execute_query(
                r#"
        {
          fieldWithNullableStringInput
        }
      "#,
                Value::Null,
            );
            assert_response(&v, &json!({"data": {"fieldWithNullableStringInput": null}}));
        }

        /// it('allows nullable inputs to be omitted in a variable')
        ///
        /// Same as above: the variable is not provided, so the argument is absent.
        #[test]
        #[ignore = "#38"]
        fn allows_nullable_inputs_to_be_omitted_in_a_variable() {
            let v = execute_query(
                r#"
        query ($value: String) {
          fieldWithNullableStringInput(input: $value)
        }
      "#,
                Value::Null,
            );
            assert_response(&v, &json!({"data": {"fieldWithNullableStringInput": null}}));
        }

        /// it('allows nullable inputs to be omitted in an unlisted variable')
        ///
        /// Upstream executes without validating; greem rejects the undefined
        /// variable at parse.
        #[test]
        fn allows_nullable_inputs_to_be_omitted_in_an_unlisted_variable() {
            let v = execute_query(
                r#"
        query {
          fieldWithNullableStringInput(input: $value)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "variable `$value` is not defined",
                        "locations": [{"line": 3, "column": 47}],
                    }],
                }),
            );
        }

        /// it('allows nullable inputs to be set to null in a variable')
        #[test]
        fn allows_nullable_inputs_to_be_set_to_null_in_a_variable() {
            let v = execute_query(
                r#"
        query ($value: String) {
          fieldWithNullableStringInput(input: $value)
        }
      "#,
                json!({"value": null}),
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNullableStringInput": "null"}}),
            );
        }

        /// it('allows nullable inputs to be set to a value in a variable')
        #[test]
        fn allows_nullable_inputs_to_be_set_to_a_value_in_a_variable() {
            let v = execute_query(
                r#"
        query ($value: String) {
          fieldWithNullableStringInput(input: $value)
        }
      "#,
                json!({"value": "a"}),
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNullableStringInput": "\"a\""}}),
            );
        }

        /// it('allows nullable inputs to be set to a value directly')
        #[test]
        fn allows_nullable_inputs_to_be_set_to_a_value_directly() {
            let v = execute_query(
                r#"
        {
          fieldWithNullableStringInput(input: "a")
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNullableStringInput": "\"a\""}}),
            );
        }
    }

    /// describe('Handles non-nullable scalars')
    mod handles_non_nullable_scalars {
        use super::*;

        /// it('allows non-nullable variable to be omitted given a default')
        #[test]
        fn allows_non_nullable_variable_to_be_omitted_given_a_default() {
            let v = execute_query(
                r#"
        query ($value: String! = "default") {
          fieldWithNullableStringInput(input: $value)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNullableStringInput": "\"default\""}}),
            );
        }

        /// it('allows non-nullable inputs to be omitted given a default')
        #[test]
        fn allows_non_nullable_inputs_to_be_omitted_given_a_default() {
            let v = execute_query(
                r#"
        query ($value: String = "default") {
          fieldWithNonNullableStringInput(input: $value)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNonNullableStringInput": "\"default\""}}),
            );
        }

        /// it('does not allow non-nullable inputs to be omitted in a variable')
        #[test]
        fn does_not_allow_non_nullable_inputs_to_be_omitted_in_a_variable() {
            let v = execute_query(
                r#"
        query ($value: String!) {
          fieldWithNonNullableStringInput(input: $value)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "missing value for non-null variable 'value'",
                        "locations": [{"line": 2, "column": 16}],
                    }],
                }),
            );
        }

        /// it('does not allow non-nullable inputs to be set to null in a variable')
        #[test]
        fn does_not_allow_non_nullable_inputs_to_be_set_to_null_in_a_variable() {
            let v = execute_query(
                r#"
        query ($value: String!) {
          fieldWithNonNullableStringInput(input: $value)
        }
      "#,
                json!({"value": null}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "null value for variable value of non-null type String!",
                    }],
                }),
            );
        }

        /// it('allows non-nullable inputs to be set to a value in a variable')
        #[test]
        fn allows_non_nullable_inputs_to_be_set_to_a_value_in_a_variable() {
            let v = execute_query(
                r#"
        query ($value: String!) {
          fieldWithNonNullableStringInput(input: $value)
        }
      "#,
                json!({"value": "a"}),
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNonNullableStringInput": "\"a\""}}),
            );
        }

        /// it('allows non-nullable inputs to be set to a value directly')
        #[test]
        fn allows_non_nullable_inputs_to_be_set_to_a_value_directly() {
            let v = execute_query(
                r#"
        {
          fieldWithNonNullableStringInput(input: "a")
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNonNullableStringInput": "\"a\""}}),
            );
        }

        /// it('reports error for missing non-nullable inputs')
        ///
        /// Upstream executes without validating and gets a execution error;
        /// greem rejects the missing required argument at parse.
        #[test]
        fn reports_error_for_missing_non_nullable_inputs() {
            let v = execute_query("{ fieldWithNonNullableStringInput }", Value::Null);
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "the required argument `Query.fieldWithNonNullableStringInput(input:)` is not provided",
                        "locations": [{"line": 1, "column": 3}],
                    }],
                }),
            );
        }

        /// it('reports error for array passed into string input')
        #[test]
        fn reports_error_for_array_passed_into_string_input() {
            let v = execute_query(
                r#"
        query ($value: String!) {
          fieldWithNonNullableStringInput(input: $value)
        }
      "#,
                json!({"value": [1, 2, 3]}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "could not coerce variable value: [1,2,3] to type String",
                    }],
                }),
            );
        }

        /// it('reports error for non-provided variables for non-nullable inputs')
        ///
        /// Upstream executes without validating and gets a execution error;
        /// greem rejects the undefined variable at parse.
        #[test]
        fn reports_error_for_non_provided_variables_for_non_nullable_inputs() {
            let v = execute_query(
                r#"
        {
          fieldWithNonNullableStringInput(input: $foo)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "variable `$foo` is not defined",
                        "locations": [{"line": 3, "column": 50}],
                    }],
                }),
            );
        }
    }

    /// describe('Handles custom scalars with embedded variables')
    mod handles_custom_scalars_with_embedded_variables {
        use super::*;

        /// it('allows custom scalars')
        #[test]
        fn allows_custom_scalars() {
            let v = execute_query(
                r#"
        {
          fieldWithJSONScalarInput(input: { a: "foo", b: ["bar"], c: "baz" })
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithJSONScalarInput": r#"{"a":"foo","b":["bar"],"c":"baz"}"#}}),
            );
        }

        /// it('allows custom scalars with non-embedded variables')
        #[test]
        fn allows_custom_scalars_with_non_embedded_variables() {
            let v = execute_query(
                r#"
          query ($input: JSONScalar) {
            fieldWithJSONScalarInput(input: $input)
          }
        "#,
                json!({"input": {"a": "foo", "b": ["bar"], "c": "baz"}}),
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithJSONScalarInput": r#"{"a":"foo","b":["bar"],"c":"baz"}"#}}),
            );
        }

        /// it('allows custom scalars with embedded operation variables')
        #[test]
        fn allows_custom_scalars_with_embedded_operation_variables() {
            let v = execute_query(
                r#"
          query ($input: String) {
            fieldWithJSONScalarInput(input: { a: $input, b: ["bar"], c: "baz" })
          }
        "#,
                json!({"input": "foo"}),
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithJSONScalarInput": r#"{"a":"foo","b":["bar"],"c":"baz"}"#}}),
            );
        }

        // it('allows custom scalars with embedded fragment variables')
        // Not ported, reason (iii): `experimentalFragmentArguments` is a graphql-js parse option; apollo-parser has no fragment variable definitions.

        // it('allows custom scalars with embedded nested fragment variables')
        // Not ported, reason (iii): `experimentalFragmentArguments` is a graphql-js parse option; apollo-parser has no fragment variable definitions.
    }

    /// describe('Handles lists and nullability')
    mod handles_lists_and_nullability {
        use super::*;

        /// it('allows lists to be null')
        #[test]
        fn allows_lists_to_be_null() {
            let v = execute_query(
                r#"
        query ($input: [String]) {
          list(input: $input)
        }
      "#,
                json!({"input": null}),
            );
            assert_response(&v, &json!({"data": {"list": "null"}}));
        }

        /// it('allows lists to contain values')
        #[test]
        fn allows_lists_to_contain_values() {
            let v = execute_query(
                r#"
        query ($input: [String]) {
          list(input: $input)
        }
      "#,
                json!({"input": ["A"]}),
            );
            assert_response(&v, &json!({"data": {"list": "[\"A\"]"}}));
        }

        /// it('allows lists to contain null')
        #[test]
        fn allows_lists_to_contain_null() {
            let v = execute_query(
                r#"
        query ($input: [String]) {
          list(input: $input)
        }
      "#,
                json!({"input": ["A", null, "B"]}),
            );
            assert_response(&v, &json!({"data": {"list": "[\"A\",null,\"B\"]"}}));
        }

        /// it('does not allow non-null lists to be null')
        #[test]
        fn does_not_allow_non_null_lists_to_be_null() {
            let v = execute_query(
                r#"
        query ($input: [String]!) {
          nnList(input: $input)
        }
      "#,
                json!({"input": null}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "null value for variable input of non-null type [String]!",
                    }],
                }),
            );
        }

        /// it('allows non-null lists to contain values')
        #[test]
        fn allows_non_null_lists_to_contain_values() {
            let v = execute_query(
                r#"
        query ($input: [String]!) {
          nnList(input: $input)
        }
      "#,
                json!({"input": ["A"]}),
            );
            assert_response(&v, &json!({"data": {"nnList": "[\"A\"]"}}));
        }

        /// it('allows non-null lists to contain null')
        #[test]
        fn allows_non_null_lists_to_contain_null() {
            let v = execute_query(
                r#"
        query ($input: [String]!) {
          nnList(input: $input)
        }
      "#,
                json!({"input": ["A", null, "B"]}),
            );
            assert_response(&v, &json!({"data": {"nnList": "[\"A\",null,\"B\"]"}}));
        }

        /// it('allows lists of non-nulls to be null')
        #[test]
        fn allows_lists_of_non_nulls_to_be_null() {
            let v = execute_query(
                r#"
        query ($input: [String!]) {
          listNN(input: $input)
        }
      "#,
                json!({"input": null}),
            );
            assert_response(&v, &json!({"data": {"listNN": "null"}}));
        }

        /// it('allows lists of non-nulls to contain values')
        #[test]
        fn allows_lists_of_non_nulls_to_contain_values() {
            let v = execute_query(
                r#"
        query ($input: [String!]) {
          listNN(input: $input)
        }
      "#,
                json!({"input": ["A"]}),
            );
            assert_response(&v, &json!({"data": {"listNN": "[\"A\"]"}}));
        }

        /// it('does not allow lists of non-nulls to contain null')
        #[test]
        fn does_not_allow_lists_of_non_nulls_to_contain_null() {
            let v = execute_query(
                r#"
        query ($input: [String!]) {
          listNN(input: $input)
        }
      "#,
                json!({"input": ["A", null, "B"]}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "null value for variable input of non-null type String!",
                    }],
                }),
            );
        }

        /// it('does not allow non-null lists of non-nulls to be null')
        #[test]
        fn does_not_allow_non_null_lists_of_non_nulls_to_be_null() {
            let v = execute_query(
                r#"
        query ($input: [String!]!) {
          nnListNN(input: $input)
        }
      "#,
                json!({"input": null}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "null value for variable input of non-null type [String!]!",
                    }],
                }),
            );
        }

        /// it('allows non-null lists of non-nulls to contain values')
        #[test]
        fn allows_non_null_lists_of_non_nulls_to_contain_values() {
            let v = execute_query(
                r#"
        query ($input: [String!]!) {
          nnListNN(input: $input)
        }
      "#,
                json!({"input": ["A"]}),
            );
            assert_response(&v, &json!({"data": {"nnListNN": "[\"A\"]"}}));
        }

        /// it('does not allow non-null lists of non-nulls to contain null')
        #[test]
        fn does_not_allow_non_null_lists_of_non_nulls_to_contain_null() {
            let v = execute_query(
                r#"
        query ($input: [String!]!) {
          nnListNN(input: $input)
        }
      "#,
                json!({"input": ["A", null, "B"]}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "null value for variable input of non-null type String!",
                    }],
                }),
            );
        }

        /// it('does not allow invalid types to be used as values')
        ///
        /// The area schema names upstream's `TestType` root `Query`.
        #[test]
        fn does_not_allow_invalid_types_to_be_used_as_values() {
            let v = execute_query(
                r#"
        query ($input: Query!) {
          fieldWithObjectInput(input: $input)
        }
      "#,
                json!({"input": {"list": ["A", "B"]}}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "`$input` variable must be of an input type",
                        "locations": [{"line": 2, "column": 16}],
                    }, {
                        "message": "variable `$input` of type `Query!` cannot be used for argument `input` of type `TestInputObject`",
                        "locations": [{"line": 3, "column": 32}],
                    }],
                }),
            );
        }

        /// it('does not allow unknown types to be used as values')
        #[test]
        fn does_not_allow_unknown_types_to_be_used_as_values() {
            let v = execute_query(
                r#"
        query ($input: UnknownType!) {
          fieldWithObjectInput(input: $input)
        }
      "#,
                json!({"input": "WhoKnows"}),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "cannot find type `UnknownType` in this document",
                        "locations": [{"line": 2, "column": 16}],
                    }, {
                        "message": "variable `$input` of type `UnknownType!` cannot be used for argument `input` of type `TestInputObject`",
                        "locations": [{"line": 3, "column": 32}],
                    }],
                }),
            );
        }
    }

    /// describe('Execute: Uses argument default values')
    mod uses_argument_default_values {
        use super::*;

        /// it('when no argument provided')
        #[test]
        fn when_no_argument_provided() {
            let v = execute_query("{ fieldWithDefaultArgumentValue }", Value::Null);
            assert_response(
                &v,
                &json!({"data": {"fieldWithDefaultArgumentValue": "\"Hello World\""}}),
            );
        }

        /// it('when omitted variable provided')
        #[test]
        fn when_omitted_variable_provided() {
            let v = execute_query(
                r#"
        query ($optional: String) {
          fieldWithDefaultArgumentValue(input: $optional)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithDefaultArgumentValue": "\"Hello World\""}}),
            );
        }

        /// it('not when argument cannot be coerced')
        ///
        /// Upstream executes without validating and gets a execution error;
        /// greem rejects the enum literal at a String argument at parse.
        #[test]
        fn not_when_argument_cannot_be_coerced() {
            let v = execute_query(
                r#"
        {
          fieldWithDefaultArgumentValue(input: WRONG_TYPE)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": "expected value of type String, found an enum",
                        "locations": [{"line": 3, "column": 48}],
                    }],
                }),
            );
        }

        /// it('when no runtime value is provided to a non-null argument')
        #[test]
        fn when_no_runtime_value_is_provided_to_a_non_null_argument() {
            let v = execute_query(
                r#"
        query optionalVariable($optional: String) {
          fieldWithNonNullableStringInputAndDefaultArgumentValue(input: $optional)
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {
                    "fieldWithNonNullableStringInputAndDefaultArgumentValue": "\"Hello World\"",
                }}),
            );
        }

        /// it('does not expose prototype argument names when omitted')
        #[test]
        fn does_not_expose_prototype_argument_names_when_omitted() {
            let v = execute_query(
                r#"
        {
          fieldWithPrototypeNamedArgument
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithPrototypeNamedArgument": "missing"}}),
            );
        }

        // it('localizes invalid default value errors during execution')
        // Not ported, reason (iii): `assumeValid` builds a schema whose argument default is invalid; the area schema is compiled as valid SDL.

        // it('localizes nested invalid field default value errors during execution')
        // Not ported, reason (iii): `assumeValid` builds a schema whose input field default is invalid; the area schema is compiled as valid SDL.
    }

    /// describe('getVariableValues: limit maximum number of coercion errors')
    mod get_variable_values_limit_maximum_number_of_coercion_errors {
        // it('return all errors by default')
        // Not ported, reason (ii): calls `getVariableValues` directly; no document-level execution.

        // it('when maxErrors is equal to number of errors')
        // Not ported, reason (ii): calls `getVariableValues` directly with `maxErrors`; no document-level execution.

        // it('when maxErrors is less than number of errors')
        // Not ported, reason (ii): calls `getVariableValues` directly with `maxErrors`; no document-level execution.
    }

    /// describe('using fragment arguments')
    ///
    /// Every case but the first parses with `experimentalFragmentArguments`,
    /// a graphql-js option; apollo-parser has no fragment variable
    /// definitions or spread arguments, so greem cannot parse them.
    mod using_fragment_arguments {
        use super::*;

        /// it('when there are no fragment arguments')
        #[test]
        fn when_there_are_no_fragment_arguments() {
            let v = execute_query(
                r#"
        query {
          ...a
        }
        fragment a on Query {
          fieldWithNonNullableStringInput(input: "A")
        }
      "#,
                Value::Null,
            );
            assert_response(
                &v,
                &json!({"data": {"fieldWithNonNullableStringInput": "\"A\""}}),
            );
        }

        // it('when a value is required and provided')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a value is required and not provided')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when the definition has a default and is provided')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when the definition has a default and is not provided')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when the definition has an invalid default and is not provided')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('does not allow invalid types to be used as fragment variables')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a definition has a default, is not provided, and spreads another fragment')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when the definition has a non-nullable default and is provided null')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when the definition has no default and is not provided')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when an argument is shadowed by an operation variable')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a nullable argument without a field default is not provided and shadowed by an operation variable')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a nullable argument with a field default is not provided and shadowed by an operation variable')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a fragment-variable is shadowed by an intermediate fragment-spread but defined in the operation-variables')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a fragment is used with different args')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when the argument variable is nested in a complex type')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when argument variables are used recursively')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when argument variables with the same name are used directly and recursively')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when argument passed in as list')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when argument passed to a directive')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when argument passed to a directive on a nested field')
        // Not ported, reason (iii): `experimentalFragmentArguments`.

        // it('when a nullable argument to a directive with a field default is not provided and shadowed by an operation variable')
        // Not ported, reason (iii): `experimentalFragmentArguments`.
    }

    /// describe('getVariableValues: own-property names')
    mod get_variable_values_own_property_names {
        // it('does not expose prototype variable names when omitted')
        // Not ported, reason (ii): inspects `getVariableValues`'s coerced map for a JS prototype name; no document-level execution.

        // it('still returns provided variables with colliding names')
        // Not ported, reason (ii): inspects `getVariableValues`'s coerced map for a JS prototype name; no document-level execution.
    }

    /// describe('getVariableValues: explicit undefined values')
    mod get_variable_values_explicit_undefined_values {
        // it('treats explicit undefined values as omitted')
        // Not ported, reason (ii): JS `undefined` in `getVariableValues`'s input; no document-level execution.
    }
}
