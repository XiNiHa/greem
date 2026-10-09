//! graphql-js v17.0.2 `src/execution/__tests__/oneof-test.ts`, case by case
//! in upstream order.
//!
//! greem does not enforce `@oneOf` yet (#42). Every case is written against
//! the spec's OneOf Input Objects coercion rule: exactly one field must be
//! provided, and its value must be non-null. The rejecting cases are ignored
//! until #42 lands; their error messages are greem's provisional wording
//! (the request-error / field-error shape, `locations`, `path` and `data`
//! are the claim, in greem's conventions: a variable error locates the
//! variable definition, a execution error locates the field).

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::oneof::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// The coercion rule every rejection cites.
const RULE: &str = "@oneOf input object `TestInputObject` requires exactly one non-null field";

/// describe('Execute: Handles OneOf Input Objects')
mod handles_oneof_input_objects {
    use super::*;

    /// describe('OneOf Input Objects')
    mod oneof_input_objects {
        use super::*;

        /// it('accepts a good default value')
        #[test]
        fn accepts_a_good_default_value() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject! = {a: "abc"}) {
          test(input: $input) {
            a
            b
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
                        "test": {
                            "a": "abc",
                            "b": null,
                        },
                    },
                }),
            );
        }

        /// it('rejects a bad default value')
        #[test]
        #[ignore = "#42"]
        fn rejects_a_bad_default_value() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject! = {a: "abc", b: 123}) {
          test(input: $input) {
            a
            b
          }
        }
      "#,
                Value::Null,
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": format!("variable `$input` got an invalid default value: {RULE}"),
                        "locations": [{"line": 2, "column": 16}],
                    }],
                }),
            );
        }

        /// it('accepts a good variable')
        #[test]
        fn accepts_a_good_variable() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject!) {
          test(input: $input) {
            a
            b
          }
        }
      "#,
                json!({"input": {"a": "abc"}}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {
                        "test": {
                            "a": "abc",
                            "b": null,
                        },
                    },
                }),
            );
        }

        /// it('accepts a good variable with an undefined key')
        ///
        /// JSON has no `undefined`: upstream's `{a: 'abc', b: undefined}` is
        /// the object without a `b` key on the wire.
        #[test]
        fn accepts_a_good_variable_with_an_undefined_key() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject!) {
          test(input: $input) {
            a
            b
          }
        }
      "#,
                json!({"input": {"a": "abc"}}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {
                        "test": {
                            "a": "abc",
                            "b": null,
                        },
                    },
                }),
            );
        }

        /// it('rejects a variable with a nulled key')
        #[test]
        #[ignore = "#42"]
        fn rejects_a_variable_with_a_nulled_key() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject!) {
          test(input: $input) {
            a
            b
          }
        }
      "#,
                json!({"input": {"a": null}}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": format!("variable `$input` got an invalid value: {RULE}"),
                        "locations": [{"line": 2, "column": 16}],
                    }],
                }),
            );
        }

        /// it('rejects a variable with multiple non-null keys')
        #[test]
        #[ignore = "#42"]
        fn rejects_a_variable_with_multiple_non_null_keys() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject!) {
          test(input: $input) {
            a
            b
          }
        }
      "#,
                json!({"input": {"a": "abc", "b": 123}}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": format!("variable `$input` got an invalid value: {RULE}"),
                        "locations": [{"line": 2, "column": 16}],
                    }],
                }),
            );
        }

        /// it('rejects a variable with multiple nullable keys')
        #[test]
        #[ignore = "#42"]
        fn rejects_a_variable_with_multiple_nullable_keys() {
            let (v, _) = World::default().single(
                r#"
        query ($input: TestInputObject!) {
          test(input: $input) {
            a
            b
          }
        }
      "#,
                json!({"input": {"a": "abc", "b": null}}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "errors": [{
                        "message": format!("variable `$input` got an invalid value: {RULE}"),
                        "locations": [{"line": 2, "column": 16}],
                    }],
                }),
            );
        }

        /// it('errors with nulled variable for field')
        ///
        /// A execution error at `test` (upstream locates the argument value,
        /// column 23; greem locates the field), so `test` is null.
        #[test]
        #[ignore = "#42"]
        fn errors_with_nulled_variable_for_field() {
            let (v, _) = World::default().single(
                r#"
        query ($a: String) {
          test(input: { a: $a }) {
            a
            b
          }
        }
      "#,
                json!({"a": null}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {
                        "test": null,
                    },
                    "errors": [{
                        "message": "argument `input` of type `TestInputObject!` was provided the variable `$a` for field `a`, which must not be null",
                        "locations": [{"line": 3, "column": 11}],
                        "path": ["test"],
                        "extensions": {"code": "BAD_USER_INPUT"},
                    }],
                }),
            );
        }

        /// it('errors with missing variable for field')
        #[test]
        #[ignore = "#42"]
        fn errors_with_missing_variable_for_field() {
            let (v, _) = World::default().single(
                r#"
        query ($a: String) {
          test(input: { a: $a }) {
            a
            b
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
                        "test": null,
                    },
                    "errors": [{
                        "message": "argument `input` of type `TestInputObject!` was provided the variable `$a` for field `a`, which was not provided a runtime value",
                        "locations": [{"line": 3, "column": 11}],
                        "path": ["test"],
                        "extensions": {"code": "BAD_USER_INPUT"},
                    }],
                }),
            );
        }

        /// it('errors with missing variable as an additional field')
        #[test]
        #[ignore = "#42"]
        fn errors_with_missing_variable_as_an_additional_field() {
            let (v, _) = World::default().single(
                r#"
        query ($a: String, $b: Int) {
          test(input: { a: $a, b: $b }) {
            a
            b
          }
        }
      "#,
                json!({"a": "abc"}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {
                        "test": null,
                    },
                    "errors": [{
                        "message": "argument `input` of type `TestInputObject!` was provided the variable `$b` for field `b`, which was not provided a runtime value",
                        "locations": [{"line": 3, "column": 11}],
                        "path": ["test"],
                        "extensions": {"code": "BAD_USER_INPUT"},
                    }],
                }),
            );
        }

        /// it('errors with nulled fragment variable for field')
        ///
        /// Also needs fragment arguments (`experimentalFragmentArguments`),
        /// which apollo-compiler 1.33 does not parse.
        #[test]
        #[ignore = "#42"]
        fn errors_with_nulled_fragment_variable_for_field() {
            let (v, _) = World::default().single(
                r#"
        query {
          ...TestFragment(a: null)
        }
        fragment TestFragment($a: String) on Query {
          test(input: { a: $a }) {
            a
            b
          }
        }
      "#,
                json!({"a": null}),
                ExecuteOptions::default(),
            );
            assert_response(
                &v,
                &json!({
                    "data": {
                        "test": null,
                    },
                    "errors": [{
                        "message": "argument `input` of type `TestInputObject!` was provided the variable `$a` for field `a`, which must not be null",
                        "locations": [{"line": 6, "column": 11}],
                        "path": ["test"],
                        "extensions": {"code": "BAD_USER_INPUT"},
                    }],
                }),
            );
        }

        /// it('errors with missing fragment variable for field')
        ///
        /// Also needs fragment arguments (`experimentalFragmentArguments`),
        /// which apollo-compiler 1.33 does not parse.
        #[test]
        #[ignore = "#42"]
        fn errors_with_missing_fragment_variable_for_field() {
            let (v, _) = World::default().single(
                r#"
        query {
          ...TestFragment
        }
        fragment TestFragment($a: String) on Query {
          test(input: { a: $a }) {
            a
            b
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
                        "test": null,
                    },
                    "errors": [{
                        "message": "argument `input` of type `TestInputObject!` was provided the variable `$a` for field `a`, which was not provided a runtime value",
                        "locations": [{"line": 6, "column": 11}],
                        "path": ["test"],
                        "extensions": {"code": "BAD_USER_INPUT"},
                    }],
                }),
            );
        }
    }
}
