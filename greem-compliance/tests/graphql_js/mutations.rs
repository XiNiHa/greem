//! graphql-js v17.0.2 `src/execution/__tests__/mutations-test.ts`, case by
//! case in upstream order.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::mutations::World;
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// describe('Execute: Handles mutation execution ordering')
mod handles_mutation_execution_ordering {
    use super::*;

    /// it('evaluates mutations serially')
    #[test]
    fn evaluates_mutations_serially() {
        let (v, _) = World::new(6).single(
            r#"
      mutation M {
        first: immediatelyChangeTheNumber(newNumber: 1) {
          theNumber
        },
        second: promiseToChangeTheNumber(newNumber: 2) {
          theNumber
        },
        third: immediatelyChangeTheNumber(newNumber: 3) {
          theNumber
        }
        fourth: promiseToChangeTheNumber(newNumber: 4) {
          theNumber
        },
        fifth: immediatelyChangeTheNumber(newNumber: 5) {
          theNumber
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
                    "first": {"theNumber": 1},
                    "second": {"theNumber": 2},
                    "third": {"theNumber": 3},
                    "fourth": {"theNumber": 4},
                    "fifth": {"theNumber": 5},
                },
            }),
        );
    }

    /// it('does not include illegal mutation fields in output')
    ///
    /// Upstream executes without validating and gets `data: {}`; greem
    /// validates at parse, so the unknown field is a request error.
    #[test]
    fn does_not_include_illegal_mutation_fields_in_output() {
        let (v, _) = World::new(6).single(
            "mutation { thisIsIllegalDoNotIncludeMe }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "type `Mutation` does not have a field `thisIsIllegalDoNotIncludeMe`",
                    "locations": [{"line": 1, "column": 12}],
                }],
            }),
        );
    }

    /// it('evaluates mutations correctly in the presence of a failed mutation')
    #[test]
    fn evaluates_mutations_correctly_in_the_presence_of_a_failed_mutation() {
        let (v, _) = World::new(6).single(
            r#"
      mutation M {
        first: immediatelyChangeTheNumber(newNumber: 1) {
          theNumber
        },
        second: promiseToChangeTheNumber(newNumber: 2) {
          theNumber
        },
        third: failToChangeTheNumber(newNumber: 3) {
          theNumber
        }
        fourth: promiseToChangeTheNumber(newNumber: 4) {
          theNumber
        },
        fifth: immediatelyChangeTheNumber(newNumber: 5) {
          theNumber
        }
        sixth: promiseAndFailToChangeTheNumber(newNumber: 6) {
          theNumber
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
                    "first": {"theNumber": 1},
                    "second": {"theNumber": 2},
                    "third": null,
                    "fourth": {"theNumber": 4},
                    "fifth": {"theNumber": 5},
                    "sixth": null,
                },
                "errors": [
                    {
                        "message": "Cannot change the number",
                        "locations": [{"line": 9, "column": 9}],
                        "path": ["third"],
                    },
                    {
                        "message": "Cannot change the number",
                        "locations": [{"line": 18, "column": 9}],
                        "path": ["sixth"],
                    },
                ],
            }),
        );
    }

    /// it('Mutation fields with @defer do not block next mutation')
    ///
    /// The fragment is released once the initial payload ships, after
    /// `second` ran, so it reads 2.
    #[test]
    fn mutation_fields_with_defer_do_not_block_next_mutation() {
        let (payloads, _) = World::new(6).run(
            r#"
      mutation M {
        first: promiseToChangeTheNumber(newNumber: 1) {
          ...DeferFragment @defer(label: "defer-label")
        },
        second: immediatelyChangeTheNumber(newNumber: 2) {
          theNumber
        }
      }
      fragment DeferFragment on NumberHolder {
        promiseToGetTheNumber
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
                        "first": {},
                        "second": {"theNumber": 2},
                    },
                    "pending": [{"id": "0", "path": ["first"], "label": "defer-label"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"promiseToGetTheNumber": 2}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Mutation inside of a fragment')
    #[test]
    fn mutation_inside_of_a_fragment() {
        let (v, _) = World::new(6).single(
            r#"
      mutation M {
        ...MutationFragment
        second: immediatelyChangeTheNumber(newNumber: 2) {
          theNumber
        }
      }
      fragment MutationFragment on Mutation {
        first: promiseToChangeTheNumber(newNumber: 1) {
          theNumber
        },
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "first": {"theNumber": 1},
                    "second": {"theNumber": 2},
                },
            }),
        );
    }

    /// it('Mutation with @defer is not executed serially')
    ///
    /// Upstream runs the deferred root field after `second`. The RFC's
    /// "Defer And Stream Directives Are Used On Valid Root Field" rule
    /// forbids `@defer` on a mutation's root selections, so greem rejects
    /// the document at parse.
    #[test]
    fn mutation_with_defer_is_not_executed_serially() {
        let (payloads, _) = World::new(6).run(
            r#"
      mutation M {
        ...MutationFragment @defer(label: "defer-label")
        second: immediatelyChangeTheNumber(newNumber: 2) {
          theNumber
        }
      }
      fragment MutationFragment on Mutation {
        first: promiseToChangeTheNumber(newNumber: 1) {
          theNumber
        },
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "`@defer` is not allowed on root selections of mutation operations",
                    "locations": [{"line": 3, "column": 29}],
                }],
            })],
        );
    }
}
