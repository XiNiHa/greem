//! graphql-js v17.0.2 `src/execution/__tests__/errorPropagation-test.ts`,
//! case by case in upstream order.

use crate::common::*;
use greem::{ErrorBehavior, ExecuteOptions};
use greem_compliance::graphql_js::error_propagation::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// describe('Execute: handles errors')
mod handles_errors {
    use super::*;

    /// it('with `@experimental_disableErrorPropagation returns null')
    ///
    /// The directive is `ErrorBehavior::Null` in greem and is not in the
    /// area schema, so the query is written without it and the option set;
    /// `foo` keeps its upstream line and column.
    #[test]
    fn with_experimental_disable_error_propagation_returns_null() {
        let (v, _) = World::default().single(
            r#"
      query getFoo {
        foo
      }
    "#,
            Value::Null,
            ExecuteOptions {
                error_behavior: ErrorBehavior::Null,
                ..Default::default()
            },
        );
        assert_response(
            &v,
            &json!({
                "data": {"foo": null},
                "errors": [{
                    "message": "bar",
                    "locations": [{"line": 3, "column": 9}],
                    "path": ["foo"],
                }],
            }),
        );
    }

    /// it('without `experimental_disableErrorPropagation` propagates the error')
    #[test]
    fn without_experimental_disable_error_propagation_propagates_the_error() {
        let (v, _) = World::default().single(
            r#"
      query getFoo {
        foo
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": null,
                "errors": [{
                    "message": "bar",
                    "locations": [{"line": 3, "column": 9}],
                    "path": ["foo"],
                }],
            }),
        );
    }
}
