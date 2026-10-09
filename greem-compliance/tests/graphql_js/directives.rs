//! graphql-js v17.0.2 `src/execution/__tests__/directives-test.ts`, case by case in upstream order.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::directives::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// Upstream's `executeTestQuery` over `rootValue = { a: 'a', b: 'b' }`.
fn execute_test_query(query: &str) -> Value {
    World::new("a", "b")
        .single(query, Value::Null, ExecuteOptions::default())
        .0
}

/// describe('works without directives')
mod works_without_directives {
    use super::*;

    /// it('basic query works')
    #[test]
    fn basic_query_works() {
        let v = execute_test_query("{ a, b }");
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }
}

/// describe('works on scalars')
mod works_on_scalars {
    use super::*;

    /// it('if true includes scalar')
    #[test]
    fn if_true_includes_scalar() {
        let v = execute_test_query("{ a, b @include(if: true) }");
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('if false omits on scalar')
    #[test]
    fn if_false_omits_on_scalar() {
        let v = execute_test_query("{ a, b @include(if: false) }");
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }

    /// it('unless false includes scalar')
    #[test]
    fn unless_false_includes_scalar() {
        let v = execute_test_query("{ a, b @skip(if: false) }");
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless true omits scalar')
    #[test]
    fn unless_true_omits_scalar() {
        let v = execute_test_query("{ a, b @skip(if: true) }");
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }
}

/// describe('works on fragment spreads')
mod works_on_fragment_spreads {
    use super::*;

    /// it('if false omits fragment spread')
    #[test]
    fn if_false_omits_fragment_spread() {
        let v = execute_test_query(
            r#"
        query {
          a
          ...Frag @include(if: false)
        }
        fragment Frag on TestType {
          b
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }

    /// it('if true includes fragment spread')
    #[test]
    fn if_true_includes_fragment_spread() {
        let v = execute_test_query(
            r#"
        query {
          a
          ...Frag @include(if: true)
        }
        fragment Frag on TestType {
          b
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless false includes fragment spread')
    #[test]
    fn unless_false_includes_fragment_spread() {
        let v = execute_test_query(
            r#"
        query {
          a
          ...Frag @skip(if: false)
        }
        fragment Frag on TestType {
          b
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless true omits fragment spread')
    #[test]
    fn unless_true_omits_fragment_spread() {
        let v = execute_test_query(
            r#"
        query {
          a
          ...Frag @skip(if: true)
        }
        fragment Frag on TestType {
          b
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }
}

/// describe('works on inline fragment')
mod works_on_inline_fragment {
    use super::*;

    /// it('if false omits inline fragment')
    #[test]
    fn if_false_omits_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... on TestType @include(if: false) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }

    /// it('if true includes inline fragment')
    #[test]
    fn if_true_includes_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... on TestType @include(if: true) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless false includes inline fragment')
    #[test]
    fn unless_false_includes_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... on TestType @skip(if: false) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless true includes inline fragment')
    #[test]
    fn unless_true_includes_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... on TestType @skip(if: true) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }
}

/// describe('works on anonymous inline fragment')
mod works_on_anonymous_inline_fragment {
    use super::*;

    /// it('if false omits anonymous inline fragment')
    #[test]
    fn if_false_omits_anonymous_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... @include(if: false) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }

    /// it('if true includes anonymous inline fragment')
    #[test]
    fn if_true_includes_anonymous_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... @include(if: true) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless false includes anonymous inline fragment')
    #[test]
    fn unless_false_includes_anonymous_inline_fragment() {
        let v = execute_test_query(
            r#"
        query Q {
          a
          ... @skip(if: false) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('unless true includes anonymous inline fragment')
    #[test]
    fn unless_true_includes_anonymous_inline_fragment() {
        let v = execute_test_query(
            r#"
        query {
          a
          ... @skip(if: true) {
            b
          }
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }
}

/// describe('works with skip and include directives')
mod works_with_skip_and_include_directives {
    use super::*;

    /// it('include and no skip')
    #[test]
    fn include_and_no_skip() {
        let v = execute_test_query(
            r#"
        {
          a
          b @include(if: true) @skip(if: false)
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a", "b": "b"}}));
    }

    /// it('include and skip')
    #[test]
    fn include_and_skip() {
        let v = execute_test_query(
            r#"
        {
          a
          b @include(if: true) @skip(if: true)
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }

    /// it('no include or skip')
    #[test]
    fn no_include_or_skip() {
        let v = execute_test_query(
            r#"
        {
          a
          b @include(if: false) @skip(if: false)
        }
      "#,
        );
        assert_response(&v, &json!({"data": {"a": "a"}}));
    }
}
