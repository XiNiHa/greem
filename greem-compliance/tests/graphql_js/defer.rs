//! graphql-js v17.0.2 `src/execution/incremental/__tests__/defer-test.ts`, case by case in upstream order.
//!
//! Every expectation is greem's payload sequence derived from `docs/spec.md`;
//! where upstream batches differently, its sequence is in an `// upstream:`
//! comment. Two rules shape most of the differences:
//!
//! - a nested fragment is announced in the payload that delivers its
//!   enclosing fragment and released after it ships, so it arrives one
//!   payload later than upstream delivers it;
//! - wire ids are assigned at announcement in tree order (the root's groups
//!   before its children's), where upstream numbers fragments in document
//!   order.
//!
//! Fields are partitioned as the RFC's `BuildExecutionPlan` does: once
//! `hero` is shared by two fragments, `hero { id }` of one and `hero { name }`
//! of the other ship under their own ids with a `subPath`, and a non-null
//! error in one of them fails that fragment alone. A fragment with no field
//! set of its own is never announced; its nested fragments are announced in
//! its place.
//!
//! Resolvers resolve in lockstep generations, so the payload sequence does
//! not depend on which resolver answers first: upstream's "promise", "slow"
//! and "null first"/"value first" variants port with the timing expressed as
//! yields (`World::slow`) and assert the same sequence as their siblings.
//! Upstream's `null` at a non-null position is a resolver error here, with
//! upstream's message text.

use crate::common::*;
use greem_compliance::graphql_js::defer::{
    A, AnotherNestedObject, B, C, DeeperObject, E, Friend, G, Hero, LateParent, LateSide,
    NestedObject, World,
};
use greem_compliance::harness::Area;
use serde_json::{Value, json};

const HERO_NON_NULL_NAME: &str = "Cannot return null for non-nullable field Hero.nonNullName.";
const FRIEND_NON_NULL_NAME: &str = "Cannot return null for non-nullable field Friend.nonNullName.";
const A_NON_NULL_ERROR_FIELD: &str =
    "Cannot return null for non-nullable field a.nonNullErrorField.";
const C_NON_NULL_ERROR_FIELD: &str =
    "Cannot return null for non-nullable field c.nonNullErrorField.";
/// A non-null leaf the case never selects.
const UNSET: &str = "unset";

/// describe('Execute: defer directive')
mod execute_defer_directive {
    use super::*;

    /// it('Can defer fragments containing scalar types')
    #[test]
    fn can_defer_fragments_containing_scalar_types() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
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
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"name": "Luke"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Returns label from defer directive')
    #[test]
    fn returns_label_from_defer_directive() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer(label: "defer-label")
        }
      }
      fragment NameFragment on Hero {
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
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"], "label": "defer-label"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"name": "Luke"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Treats null defer label the same as no label')
    #[test]
    fn treats_null_defer_label_the_same_as_no_label() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer(label: null)
        }
      }
      fragment NameFragment on Hero {
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
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"name": "Luke"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can disable defer using if argument')
    #[test]
    fn can_disable_defer_using_if_argument() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer(if: false)
        }
      }
      fragment NameFragment on Hero {
        name
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({"data": {"hero": {"id": "1", "name": "Luke"}}})],
        );
    }

    /// it('Does not disable defer with null if argument')
    #[test]
    fn does_not_disable_defer_with_null_if_argument() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery($shouldDefer: Boolean) {
        hero {
          id
          ...NameFragment @defer(if: $shouldDefer)
        }
      }
      fragment NameFragment on Hero {
        name
      }
    "#,
            json!({}),
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"name": "Luke"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    // it('Does not execute deferred fragments early when not specified')
    // Not ported, reason (iii): asserts the resolver order `enableEarlyExecution: false` gives.

    // it('Does execute deferred fragments early when specified')
    // Not ported, reason (iii): asserts the resolver order `enableEarlyExecution: true` gives.

    /// it('Can defer fragments on the top level Query field')
    #[test]
    fn can_defer_fragments_on_the_top_level_query_field() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        ...QueryFragment @defer(label: "DeferQuery")
      }
      fragment QueryFragment on Query {
        hero {
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
                    "data": {},
                    "pending": [{"id": "0", "path": [], "label": "DeferQuery"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"hero": {"id": "1"}}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can defer fragments with errors on the top level Query field')
    #[test]
    fn can_defer_fragments_with_errors_on_the_top_level_query_field() {
        let (payloads, _) = World::with_hero(Hero {
            name: Err("bad"),
            ..Hero::luke()
        })
        .run(
            r#"
      query HeroNameQuery {
        ...QueryFragment @defer(label: "DeferQuery")
      }
      fragment QueryFragment on Query {
        hero {
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
                    "data": {},
                    "pending": [{"id": "0", "path": [], "label": "DeferQuery"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "data": {"hero": {"name": null}},
                        "errors": [{
                            "message": "bad",
                            "locations": [{"line": 7, "column": 11}],
                            "path": ["hero", "name"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can defer a fragment within an already deferred fragment')
    #[test]
    fn can_defer_a_fragment_within_an_already_deferred_fragment() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ...TopFragment @defer(label: "DeferTop")
        }
      }
      fragment TopFragment on Hero {
        id
        ...NestedFragment @defer(label: "DeferNested")
      }
      fragment NestedFragment on Hero {
        friends {
          name
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: one payload announces DeferNested and delivers both
        // fragments; greem releases the nested one after that payload ships.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {}},
                    "pending": [{"id": "0", "path": ["hero"], "label": "DeferTop"}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "1", "path": ["hero"], "label": "DeferNested"}],
                    "incremental": [{"id": "0", "data": {"id": "1"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "1",
                        "data": {"friends": [{"name": "Han"}, {"name": "Leia"}, {"name": "C-3PO"}]},
                    }],
                    "completed": [{"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can defer a fragment that is also not deferred, deferred fragment is first')
    #[test]
    fn can_defer_a_fragment_that_is_also_not_deferred_deferred_fragment_is_first() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ...TopFragment @defer(label: "DeferTop")
          ...TopFragment
        }
      }
      fragment TopFragment on Hero {
        name
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(&payloads, &[json!({"data": {"hero": {"name": "Luke"}}})]);
    }

    /// it('Can defer a fragment that is also not deferred, non-deferred fragment is first')
    #[test]
    fn can_defer_a_fragment_that_is_also_not_deferred_non_deferred_fragment_is_first() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ...TopFragment
          ...TopFragment @defer(label: "DeferTop")
        }
      }
      fragment TopFragment on Hero {
        name
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(&payloads, &[json!({"data": {"hero": {"name": "Luke"}}})]);
    }

    /// it('Can defer an inline fragment')
    #[test]
    fn can_defer_an_inline_fragment() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ... on Hero @defer(label: "InlineDeferred") {
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
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"], "label": "InlineDeferred"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"name": "Luke"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Does not emit empty defer fragments')
    ///
    /// Upstream executes without validating and gets `{ hero: {} }`; greem
    /// validates at parse, where the unused `TopFragment` is a request error
    /// (apollo-compiler's wording, read off a run).
    #[test]
    fn does_not_emit_empty_defer_fragments() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ... @defer {
            name @skip(if: true)
          }
        }
      }
      fragment TopFragment on Hero {
        name
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "fragment `TopFragment` must be used in an operation",
                    "locations": [{"line": 9, "column": 7}],
                }],
            })],
        );
    }

    /// it('Emits children of empty defer fragments')
    #[test]
    fn emits_children_of_empty_defer_fragments() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ... @defer {
            ... @defer {
              name
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
                    "data": {"hero": {}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"name": "Luke"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can separately emit defer fragments with different labels with varying fields')
    #[test]
    fn can_separately_emit_defer_fragments_with_different_labels_with_varying_fields() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ... @defer(label: "DeferID") {
            id
          }
          ... @defer(label: "DeferName") {
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
                    "data": {"hero": {}},
                    "pending": [
                        {"id": "0", "path": ["hero"], "label": "DeferID"},
                        {"id": "1", "path": ["hero"], "label": "DeferName"},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"id": "1"}},
                        {"id": "1", "data": {"name": "Luke"}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Separately emits defer fragments with different labels with varying subfields')
    #[test]
    fn separately_emits_defer_fragments_with_different_labels_with_varying_subfields() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        ... @defer(label: "DeferID") {
          hero {
            id
          }
        }
        ... @defer(label: "DeferName") {
          hero {
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
                    "data": {},
                    "pending": [
                        {"id": "0", "path": [], "label": "DeferID"},
                        {"id": "1", "path": [], "label": "DeferName"},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"hero": {}}},
                        {"id": "0", "subPath": ["hero"], "data": {"id": "1"}},
                        {"id": "1", "subPath": ["hero"], "data": {"name": "Luke"}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Separately emits defer fragments with different labels with varying subfields that return promises')
    #[test]
    fn separately_emits_defer_fragments_with_different_labels_with_varying_subfields_that_return_promises()
     {
        let (payloads, _) = World {
            slow: vec!["Hero.id", "Hero.name"],
            ..World::with_hero(Hero::luke())
        }
        .run(
            r#"
      query HeroNameQuery {
        ... @defer(label: "DeferID") {
          hero {
            id
          }
        }
        ... @defer(label: "DeferName") {
          hero {
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
                    "data": {},
                    "pending": [
                        {"id": "0", "path": [], "label": "DeferID"},
                        {"id": "1", "path": [], "label": "DeferName"},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"hero": {}}},
                        {"id": "0", "subPath": ["hero"], "data": {"id": "1"}},
                        {"id": "1", "subPath": ["hero"], "data": {"name": "Luke"}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Separately emits defer fragments with varying subfields of same priorities but different level of defers')
    #[test]
    fn separately_emits_defer_fragments_with_varying_subfields_of_same_priorities_but_different_level_of_defers()
     {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          ... @defer(label: "DeferID") {
            id
          }
        }
        ... @defer(label: "DeferName") {
          hero {
            name
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: DeferID is id 0 and DeferName id 1, in document order;
        // greem announces the root's group before the hero object's.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {}},
                    "pending": [
                        {"id": "0", "path": [], "label": "DeferName"},
                        {"id": "1", "path": ["hero"], "label": "DeferID"},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "subPath": ["hero"], "data": {"name": "Luke"}},
                        {"id": "1", "data": {"id": "1"}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Separately emits nested defer fragments with varying subfields of same priorities but different level of defers')
    #[test]
    fn separately_emits_nested_defer_fragments_with_varying_subfields_of_same_priorities_but_different_level_of_defers()
     {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        ... @defer(label: "DeferName") {
          hero {
            name
            ... @defer(label: "DeferID") {
              id
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: DeferID is announced and delivered in the same payload
        // as DeferName; greem releases it after that payload ships.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {},
                    "pending": [{"id": "0", "path": [], "label": "DeferName"}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "1", "path": ["hero"], "label": "DeferID"}],
                    "incremental": [{"id": "0", "data": {"hero": {"name": "Luke"}}}],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "1", "data": {"id": "1"}}],
                    "completed": [{"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Initiates deferred grouped field sets only if they have been released as pending')
    ///
    /// Upstream holds `someField` on a promise it resolves between payloads
    /// and spies on when `c` and `e` run; the spies are per-object claims and
    /// are not asserted. The slow field is a yield here, and the sequence does
    /// not depend on it: each fragment ships at the barrier its work finishes.
    #[test]
    fn initiates_deferred_grouped_field_sets_only_if_they_have_been_released_as_pending() {
        let (payloads, _) = World {
            a: Some(A {
                b: Some(B {
                    c: Some(C {
                        d: Ok(Some("d".into())),
                        non_null_error_field: Err(UNSET),
                    }),
                    e: Some(E {
                        f: Ok(Some("f".into())),
                    }),
                }),
                some_field: Ok(Some("someField".into())),
                non_null_error_field: Err(UNSET),
            }),
            slow: vec!["A.someField"],
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            ... @defer {
              b {
                c { d }
              }
            }
          }
        }
        ... @defer {
          a {
            someField
            ... @defer {
              b {
                e { f }
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: three payloads; the second announces id 2 and already
        // delivers `b` and `c { d }` under it, the third announces id 3 and
        // delivers `someField` under 1 and `e { f }` under 3. greem releases
        // each nested fragment after its announcement ships.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "2", "path": ["a"]}],
                    "incremental": [{"id": "0", "data": {"a": {}}}],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "3", "path": ["a"]}],
                    "incremental": [{"id": "1", "subPath": ["a"], "data": {"someField": "someField"}}],
                    "completed": [{"id": "1"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "2", "data": {"b": {}}},
                        {"id": "2", "subPath": ["b"], "data": {"c": {"d": "d"}}},
                        {"id": "3", "subPath": ["b"], "data": {"e": {"f": "f"}}},
                    ],
                    "completed": [{"id": "2"}, {"id": "3"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Initiates unique deferred grouped field sets after those that are common to sibling defers')
    ///
    /// Upstream's resolver spies are not asserted; `c` is slow by a yield.
    #[test]
    fn initiates_unique_deferred_grouped_field_sets_after_those_that_are_common_to_sibling_defers()
    {
        let (payloads, _) = World {
            a: Some(A {
                b: Some(B {
                    c: Some(C {
                        d: Ok(Some("d".into())),
                        non_null_error_field: Err(UNSET),
                    }),
                    e: Some(E {
                        f: Ok(Some("f".into())),
                    }),
                }),
                some_field: Ok(None),
                non_null_error_field: Err(UNSET),
            }),
            slow: vec!["B.c"],
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            ... @defer {
              b {
                c { d }
              }
            }
          }
        }
        ... @defer {
          a {
            ... @defer {
              b {
                c { d }
                e { f }
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the last payload delivers `{ b: { c: { d } } }` as one
        // entry under 2; greem ships the `b` set and the `c { d }` set, each
        // shared by 2 and 3, as separate entries under the first member to
        // complete.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "2", "path": ["a"]}, {"id": "3", "path": ["a"]}],
                    "incremental": [{"id": "0", "data": {"a": {}}}],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "2", "data": {"b": {"c": {"d": "d"}}}},
                        {"id": "3", "subPath": ["b"], "data": {"e": {"f": "f"}}},
                    ],
                    "completed": [{"id": "2"}, {"id": "3"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Can deduplicate multiple defers on the same object')
    ///
    /// The nested fragments select nothing of their own (every field is the
    /// outermost one's by the ancestor rule), so only the outermost is
    /// announced, once per friend.
    #[test]
    fn can_deduplicate_multiple_defers_on_the_same_object() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query {
        hero {
          friends {
            ... @defer {
              ...FriendFrag
              ... @defer {
                ...FriendFrag
                ... @defer {
                  ...FriendFrag
                  ... @defer {
                    ...FriendFrag
                  }
                }
              }
            }
          }
        }
      }

      fragment FriendFrag on Friend {
        id
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
                    "data": {"hero": {"friends": [{}, {}, {}]}},
                    "pending": [
                        {"id": "0", "path": ["hero", "friends", 0]},
                        {"id": "1", "path": ["hero", "friends", 1]},
                        {"id": "2", "path": ["hero", "friends", 2]},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"id": "2", "name": "Han"}},
                        {"id": "1", "data": {"id": "3", "name": "Leia"}},
                        {"id": "2", "data": {"id": "4", "name": "C-3PO"}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}, {"id": "2"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicates fields present in the initial payload')
    #[test]
    fn deduplicates_fields_present_in_the_initial_payload() {
        let (payloads, _) = World::with_hero(Hero {
            nested_object: Some(NestedObject {
                deeper_object: Some(DeeperObject {
                    foo: Ok(Some("foo".into())),
                    bar: Ok(Some("bar".into())),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            another_nested_object: Some(AnotherNestedObject {
                deeper_object: Some(DeeperObject {
                    foo: Ok(Some("foo".into())),
                    ..Default::default()
                }),
            }),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          nestedObject {
            deeperObject {
              foo
            }
          }
          anotherNestedObject {
            deeperObject {
              foo
            }
          }
          ... @defer {
            nestedObject {
              deeperObject {
                bar
              }
            }
            anotherNestedObject {
              deeperObject {
                foo
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
                    "data": {"hero": {
                        "nestedObject": {"deeperObject": {"foo": "foo"}},
                        "anotherNestedObject": {"deeperObject": {"foo": "foo"}},
                    }},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "subPath": ["nestedObject", "deeperObject"],
                        "data": {"bar": "bar"},
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicates fields present in a parent defer payload')
    #[test]
    fn deduplicates_fields_present_in_a_parent_defer_payload() {
        let (payloads, _) = World::with_hero(Hero {
            nested_object: Some(NestedObject {
                deeper_object: Some(DeeperObject {
                    foo: Ok(Some("foo".into())),
                    bar: Ok(Some("bar".into())),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          ... @defer {
            nestedObject {
              deeperObject {
                foo
                ... @defer {
                  foo
                  bar
                }
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the nested fragment is announced and delivered in the
        // same payload as its parent.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "1", "path": ["hero", "nestedObject", "deeperObject"]}],
                    "incremental": [{
                        "id": "0",
                        "data": {"nestedObject": {"deeperObject": {"foo": "foo"}}},
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "1", "data": {"bar": "bar"}}],
                    "completed": [{"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicates fields with deferred fragments at multiple levels')
    #[test]
    fn deduplicates_fields_with_deferred_fragments_at_multiple_levels() {
        let (payloads, _) = World::with_hero(Hero {
            nested_object: Some(NestedObject {
                deeper_object: Some(DeeperObject {
                    foo: Ok(Some("foo".into())),
                    bar: Ok(Some("bar".into())),
                    baz: Ok(Some("baz".into())),
                    bak: Ok(Some("bak".into())),
                }),
                ..Default::default()
            }),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          nestedObject {
            deeperObject {
              foo
            }
          }
          ... @defer {
            nestedObject {
              deeperObject {
                foo
                bar
              }
              ... @defer {
                deeperObject {
                  foo
                  bar
                  baz
                  ... @defer {
                    foo
                    bar
                    baz
                    bak
                  }
                }
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: one payload after the initial one announces ids 1 and 2
        // and delivers all three fragments; greem releases each nested
        // fragment after the payload delivering its parent ships.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {"nestedObject": {"deeperObject": {"foo": "foo"}}}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "1", "path": ["hero", "nestedObject"]}],
                    "incremental": [{
                        "id": "0",
                        "subPath": ["nestedObject", "deeperObject"],
                        "data": {"bar": "bar"},
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [{"id": "2", "path": ["hero", "nestedObject", "deeperObject"]}],
                    "incremental": [{"id": "1", "subPath": ["deeperObject"], "data": {"baz": "baz"}}],
                    "completed": [{"id": "1"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "2", "data": {"bak": "bak"}}],
                    "completed": [{"id": "2"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicates multiple fields from deferred fragments from different branches occurring at the same level')
    ///
    /// The fragment on `hero` selects nothing of its own (`nestedObject` and
    /// `deeperObject` are immediate), so it is not announced and the fragment
    /// nested in it is announced in the initial payload in its place. `foo`
    /// is shared by both announced fragments.
    #[test]
    fn deduplicates_multiple_fields_from_deferred_fragments_from_different_branches_occurring_at_the_same_level()
     {
        let (payloads, _) = World::with_hero(Hero {
            nested_object: Some(NestedObject {
                deeper_object: Some(DeeperObject {
                    foo: Ok(Some("foo".into())),
                    bar: Ok(Some("bar".into())),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          nestedObject {
            deeperObject {
              ... @defer {
                foo
              }
            }
          }
          ... @defer {
            nestedObject {
              deeperObject {
                ... @defer {
                  foo
                  bar
                }
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
                    "data": {"hero": {"nestedObject": {"deeperObject": {}}}},
                    "pending": [
                        {"id": "0", "path": ["hero", "nestedObject", "deeperObject"]},
                        {"id": "1", "path": ["hero", "nestedObject", "deeperObject"]},
                    ],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"foo": "foo"}},
                        {"id": "1", "data": {"bar": "bar"}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicate fields with deferred fragments in different branches at multiple non-overlapping levels')
    #[test]
    fn deduplicate_fields_with_deferred_fragments_in_different_branches_at_multiple_non_overlapping_levels()
     {
        let (payloads, _) = World {
            a: Some(A {
                b: Some(B {
                    c: Some(C {
                        d: Ok(Some("d".into())),
                        non_null_error_field: Err(UNSET),
                    }),
                    e: Some(E {
                        f: Ok(Some("f".into())),
                    }),
                }),
                some_field: Ok(None),
                non_null_error_field: Err(UNSET),
            }),
            g: Some(G {
                h: Ok(Some("h".into())),
            }),
            ..Default::default()
        }
        .run(
            r#"
      query {
        a {
          b {
            c {
              d
            }
            ... @defer {
              e {
                f
              }
            }
          }
        }
        ... @defer {
          a {
            b {
              e {
                f
              }
            }
          }
          g {
            h
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the fragment inside `b` is id 0 and ships `e`, the root
        // one is id 1 and ships `g`; greem announces the root's group first
        // and ships the shared `e { f }` set under the first member to
        // complete, which is the lower id.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"a": {"b": {"c": {"d": "d"}}}},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": ["a", "b"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"g": {"h": "h"}}},
                        {"id": "0", "subPath": ["a", "b"], "data": {"e": {"f": "f"}}},
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Correctly bundles varying subfields into incremental data records unique by defer combination, ignoring fields in a fragment masked by a parent defer')
    ///
    /// The innermost fragment selects nothing of its own, so it is never
    /// announced.
    #[test]
    fn correctly_bundles_varying_subfields_into_incremental_data_records_unique_by_defer_combination_ignoring_fields_in_a_fragment_masked_by_a_parent_defer()
     {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        ... @defer {
          hero {
            id
          }
        }
        ... @defer {
          hero {
            name
            shouldBeWithNameDespiteAdditionalDefer: name
            ... @defer {
              shouldBeWithNameDespiteAdditionalDefer: name
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
                    "data": {},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"hero": {}}},
                        {"id": "0", "subPath": ["hero"], "data": {"id": "1"}},
                        {
                            "id": "1",
                            "subPath": ["hero"],
                            "data": {
                                "name": "Luke",
                                "shouldBeWithNameDespiteAdditionalDefer": "Luke",
                            },
                        },
                    ],
                    "completed": [{"id": "0"}, {"id": "1"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// Upstream's `{ a: { b: { c: { d: 'd' } }, someField: 'someField' } }`
    /// with `c.nonNullErrorField` failing.
    fn a_with_failing_c() -> A {
        A {
            b: Some(B {
                c: Some(C {
                    d: Ok(Some("d".into())),
                    non_null_error_field: Err(C_NON_NULL_ERROR_FIELD),
                }),
                e: None,
            }),
            some_field: Ok(Some("someField".into())),
            non_null_error_field: Err(UNSET),
        }
    }

    /// it('Nulls cross defer boundaries, null first')
    ///
    /// The non-null error's nearest nullable ancestor `c` is outside the
    /// failing fragment's own field set, so that fragment fails; the fragment
    /// inside `a` still ships the `b { c }` set both share, under its own id.
    #[test]
    fn nulls_cross_defer_boundaries_null_first() {
        let (payloads, _) = World {
            a: Some(a_with_failing_c()),
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            someField
            b {
              c {
                nonNullErrorField
              }
            }
          }
        }
        a {
          ... @defer {
            b {
              c {
                d
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the fragment inside `a` is id 0 and the root one id 1;
        // greem announces the root's group first.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"a": {}},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": ["a"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "1", "data": {"b": {"c": {}}}},
                        {"id": "1", "subPath": ["b", "c"], "data": {"d": "d"}},
                    ],
                    "completed": [
                        {
                            "id": "0",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 8, "column": 17}],
                                "path": ["a", "b", "c", "nonNullErrorField"],
                            }],
                        },
                        {"id": "1"},
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Nulls cross defer boundaries, value first')
    ///
    /// The mirror of the case above: here the root fragment survives and
    /// ships the shared `b { c }` set. Both settle at one barrier whichever
    /// resolver answers first.
    #[test]
    fn nulls_cross_defer_boundaries_value_first() {
        let (payloads, _) = World {
            a: Some(a_with_failing_c()),
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            b {
              c {
                d
              }
            }
          }
        }
        a {
          ... @defer {
            someField
            b {
              c {
                nonNullErrorField
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the fragment inside `a` is id 0 and the root one id 1.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"a": {}},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": ["a"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "subPath": ["a"], "data": {"b": {"c": {}}}},
                        {"id": "0", "subPath": ["a", "b", "c"], "data": {"d": "d"}},
                    ],
                    "completed": [
                        {"id": "0"},
                        {
                            "id": "1",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 17, "column": 17}],
                                "path": ["a", "b", "c", "nonNullErrorField"],
                            }],
                        },
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Nulls cross defer boundaries, failed fragment with slower shared child execution groups')
    #[test]
    fn nulls_cross_defer_boundaries_failed_fragment_with_slower_shared_child_execution_groups() {
        let (payloads, _) = World {
            a: Some(A {
                b: Some(B {
                    c: Some(C {
                        d: Ok(Some("d".into())),
                        non_null_error_field: Err(UNSET),
                    }),
                    e: Some(E {
                        f: Ok(Some("f".into())),
                    }),
                }),
                some_field: Ok(Some("someField".into())),
                non_null_error_field: Err(A_NON_NULL_ERROR_FIELD),
            }),
            slow: vec!["A.someField"],
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            someField
            nonNullErrorField
            b {
              c {
                d
              }
            }
          }
        }
        a {
          ... @defer {
            someField
            b {
              e {
                f
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: ids swapped, and the failure ships in a payload of its
        // own before the surviving fragment's data because its shared child
        // groups were slower; greem settles both fragments at one barrier,
        // the failing one once nothing beneath it is live.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"a": {}},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": ["a"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "1", "data": {"someField": "someField", "b": {}}},
                        {"id": "1", "subPath": ["b"], "data": {"e": {"f": "f"}}},
                    ],
                    "completed": [
                        {
                            "id": "0",
                            "errors": [{
                                "message": A_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 6, "column": 13}],
                                "path": ["a", "nonNullErrorField"],
                            }],
                        },
                        {"id": "1"},
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles cancelling child deferred fragments if parent fragment fails')
    ///
    /// As `nulls_cross_defer_boundaries_null_first`, with a nested fragment
    /// that selects nothing of its own (`someField` is its parent's) and so
    /// is never announced.
    #[test]
    fn handles_cancelling_child_deferred_fragments_if_parent_fragment_fails() {
        let (payloads, _) = World {
            a: Some(a_with_failing_c()),
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            someField
            b {
              c {
                nonNullErrorField
              }
            }
          }
          ... @defer {
            a {
              someField
            }
          }
        }
        a {
          ... @defer {
            b {
              c {
                d
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the fragment inside `a` is id 0 and ships `b { c: {} }`
        // and `d`; the root one is id 1 and fails with the error.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"a": {}},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": ["a"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "1", "data": {"b": {"c": {}}}},
                        {"id": "1", "subPath": ["b", "c"], "data": {"d": "d"}},
                    ],
                    "completed": [
                        {
                            "id": "0",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 8, "column": 17}],
                                "path": ["a", "b", "c", "nonNullErrorField"],
                            }],
                        },
                        {"id": "1"},
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles multiple erroring deferred grouped field sets')
    #[test]
    fn handles_multiple_erroring_deferred_grouped_field_sets() {
        let (payloads, _) = World {
            a: Some(A {
                b: Some(B {
                    c: Some(C {
                        d: Ok(None),
                        non_null_error_field: Err(C_NON_NULL_ERROR_FIELD),
                    }),
                    e: None,
                }),
                some_field: Ok(None),
                non_null_error_field: Err(UNSET),
            }),
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            b {
              c {
                someError: nonNullErrorField
              }
            }
          }
        }
        ... @defer {
          a {
            b {
              c {
                anotherError: nonNullErrorField
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
                    "data": {},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [
                        {
                            "id": "0",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 7, "column": 17}],
                                "path": ["a", "b", "c", "someError"],
                            }],
                        },
                        {
                            "id": "1",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 16, "column": 17}],
                                "path": ["a", "b", "c", "anotherError"],
                            }],
                        },
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles multiple erroring deferred grouped field sets for the same fragment')
    ///
    /// The failed fragment reports the first error that reached its boundary
    /// in response order.
    #[test]
    fn handles_multiple_erroring_deferred_grouped_field_sets_for_the_same_fragment() {
        let (payloads, _) = World {
            a: Some(a_with_failing_c()),
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            b {
              someC: c {
                d: d
              }
              anotherC: c {
                d: d
              }
            }
          }
        }
        ... @defer {
          a {
            b {
              someC: c {
                someError: nonNullErrorField
              }
              anotherC: c {
                anotherError: nonNullErrorField
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
                    "data": {},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "data": {"a": {"b": {"someC": {}, "anotherC": {}}}}},
                        {"id": "0", "subPath": ["a", "b", "someC"], "data": {"d": "d"}},
                        {"id": "0", "subPath": ["a", "b", "anotherC"], "data": {"d": "d"}},
                    ],
                    "completed": [
                        {"id": "0"},
                        {
                            "id": "1",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 19, "column": 17}],
                                "path": ["a", "b", "someC", "someError"],
                            }],
                        },
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('filters a payload with a null that cannot be merged')
    ///
    /// `nulls_cross_defer_boundaries_null_first` with the failing field slow
    /// by a yield.
    #[test]
    fn filters_a_payload_with_a_null_that_cannot_be_merged() {
        let (payloads, _) = World {
            a: Some(a_with_failing_c()),
            slow: vec!["C.nonNullErrorField"],
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          a {
            someField
            b {
              c {
                nonNullErrorField
              }
            }
          }
        }
        a {
          ... @defer {
            b {
              c {
                d
              }
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: ids swapped, and the surviving fragment ships in a payload
        // of its own before the slower one fails; greem settles both at one
        // barrier.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"a": {}},
                    "pending": [{"id": "0", "path": []}, {"id": "1", "path": ["a"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "1", "data": {"b": {"c": {}}}},
                        {"id": "1", "subPath": ["b", "c"], "data": {"d": "d"}},
                    ],
                    "completed": [
                        {
                            "id": "0",
                            "errors": [{
                                "message": C_NON_NULL_ERROR_FIELD,
                                "locations": [{"line": 8, "column": 17}],
                                "path": ["a", "b", "c", "nonNullErrorField"],
                            }],
                        },
                        {"id": "1"},
                    ],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Cancels deferred fields when initial result exhibits null bubbling cancelling the defer')
    #[test]
    fn cancels_deferred_fields_when_initial_result_exhibits_null_bubbling_cancelling_the_defer() {
        let (payloads, _) = World::with_hero(Hero {
            non_null_name: Err(HERO_NON_NULL_NAME),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
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
            &[json!({
                "data": {"hero": null},
                "errors": [{
                    "message": HERO_NON_NULL_NAME,
                    "locations": [{"line": 4, "column": 11}],
                    "path": ["hero", "nonNullName"],
                }],
            })],
        );
    }

    /// it('Cancels deferred fields when initial result exhibits null bubbling cancelling new fields')
    ///
    /// The root fragment's only field set sits under the nulled `hero`, so
    /// nothing of it can be delivered and it is not announced.
    #[test]
    fn cancels_deferred_fields_when_initial_result_exhibits_null_bubbling_cancelling_new_fields() {
        let (payloads, _) = World::with_hero(Hero {
            non_null_name: Err(HERO_NON_NULL_NAME),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          nonNullName
        }
        ... @defer {
          hero {
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
                "data": {"hero": null},
                "errors": [{
                    "message": HERO_NON_NULL_NAME,
                    "locations": [{"line": 4, "column": 11}],
                    "path": ["hero", "nonNullName"],
                }],
            })],
        );
    }

    /// it('Keeps deferred work outside nulled error paths')
    #[test]
    fn keeps_deferred_work_outside_nulled_error_paths() {
        let (payloads, _) = World {
            a: Some(A {
                b: None,
                some_field: Ok(Some("someField".into())),
                non_null_error_field: Err(A_NON_NULL_ERROR_FIELD),
            }),
            g: Some(G {
                h: Ok(Some("value".into())),
            }),
            ..Default::default()
        }
        .run(
            r#"
      query {
        a {
          ... @defer {
            someField
          }
          nonNullErrorField
        }
        g {
          ... @defer {
            h
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
                    "data": {"a": null, "g": {}},
                    "errors": [{
                        "message": A_NON_NULL_ERROR_FIELD,
                        "locations": [{"line": 7, "column": 11}],
                        "path": ["a", "nonNullErrorField"],
                    }],
                    "pending": [{"id": "0", "path": ["g"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"h": "value"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Stops late initial-path completion before publishing a deferred response')
    ///
    /// Upstream resolves `side` only after the deferred payload and spies
    /// that `value` never runs, a per-object claim not asserted here: greem's
    /// initial payload waits for its whole generation, so `side` is slow by
    /// a yield and its value is simply dropped with the nulled `parent`.
    #[test]
    fn stops_late_initial_path_completion_before_publishing_a_deferred_response() {
        let (payloads, _) = World {
            parent: Some(LateParent {
                boom: Err("boom"),
                side: Some(LateSide {
                    value: Ok(Some("late value".into())),
                }),
            }),
            g: Some(G {
                h: Ok(Some("value".into())),
            }),
            slow: vec!["LateParent.side"],
            ..Default::default()
        }
        .run(
            r#"
      query {
        parent {
          boom
          side {
            value
          }
        }
        g {
          ... @defer {
            h
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
                    "data": {"parent": null, "g": {}},
                    "errors": [{
                        "message": "boom",
                        "locations": [{"line": 4, "column": 11}],
                        "path": ["parent", "boom"],
                    }],
                    "pending": [{"id": "0", "path": ["g"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{"id": "0", "data": {"h": "value"}}],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Cancels deferred fields when deferred result exhibits null bubbling')
    #[test]
    fn cancels_deferred_fields_when_deferred_result_exhibits_null_bubbling() {
        let (payloads, _) = World::with_hero(Hero {
            non_null_name: Err(HERO_NON_NULL_NAME),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        ... @defer {
          hero {
            nonNullName
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
                    "data": {},
                    "pending": [{"id": "0", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "data": {"hero": null},
                        "errors": [{
                            "message": HERO_NON_NULL_NAME,
                            "locations": [{"line": 5, "column": 13}],
                            "path": ["hero", "nonNullName"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Stops late deferred payload completion after deferred null bubbling')
    ///
    /// As `stops_late_initial_path_completion_before_publishing_a_deferred_response`:
    /// `side` is slow by a yield and the spy on `value` is not asserted.
    #[test]
    fn stops_late_deferred_payload_completion_after_deferred_null_bubbling() {
        let (payloads, _) = World {
            parent: Some(LateParent {
                boom: Err("boom"),
                side: Some(LateSide {
                    value: Ok(Some("late value".into())),
                }),
            }),
            slow: vec!["LateParent.side"],
            ..Default::default()
        }
        .run(
            r#"
      query {
        ... @defer {
          parent {
            side {
              value
            }
            boom
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
                    "data": {},
                    "pending": [{"id": "0", "path": []}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "data": {"parent": null},
                        "errors": [{
                            "message": "boom",
                            "locations": [{"line": 8, "column": 13}],
                            "path": ["parent", "boom"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicates list fields')
    #[test]
    fn deduplicates_list_fields() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query {
        hero {
          friends {
            name
          }
          ... @defer {
            friends {
              name
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
                "data": {"hero": {"friends": [{"name": "Han"}, {"name": "Leia"}, {"name": "C-3PO"}]}},
            })],
        );
    }

    /// it('Deduplicates async iterable list fields')
    #[test]
    fn deduplicates_async_iterable_list_fields() {
        let (payloads, _) = World::with_hero(Hero {
            friends: Some(vec![Friend::new("2", "Han")]),
            friends_async: true,
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          friends {
            name
          }
          ... @defer {
            friends {
              name
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
            &[json!({"data": {"hero": {"friends": [{"name": "Han"}]}}})],
        );
    }

    /// it('Deduplicates empty async iterable list fields')
    #[test]
    fn deduplicates_empty_async_iterable_list_fields() {
        let (payloads, _) = World::with_hero(Hero {
            friends: Some(vec![]),
            friends_async: true,
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          friends {
            name
          }
          ... @defer {
            friends {
              name
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(&payloads, &[json!({"data": {"hero": {"friends": []}}})]);
    }

    /// it('Does not deduplicate list fields with non-overlapping fields')
    #[test]
    fn does_not_deduplicate_list_fields_with_non_overlapping_fields() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query {
        hero {
          friends {
            name
          }
          ... @defer {
            friends {
              id
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
                    "data": {"hero": {"friends": [{"name": "Han"}, {"name": "Leia"}, {"name": "C-3PO"}]}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "0", "subPath": ["friends", 0], "data": {"id": "2"}},
                        {"id": "0", "subPath": ["friends", 1], "data": {"id": "3"}},
                        {"id": "0", "subPath": ["friends", 2], "data": {"id": "4"}},
                    ],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Deduplicates list fields that return empty lists')
    #[test]
    fn deduplicates_list_fields_that_return_empty_lists() {
        let (payloads, _) = World::with_hero(Hero {
            friends: Some(vec![]),
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          friends {
            name
          }
          ... @defer {
            friends {
              name
            }
          }
        }
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(&payloads, &[json!({"data": {"hero": {"friends": []}}})]);
    }

    /// it('Deduplicates null object fields')
    #[test]
    fn deduplicates_null_object_fields() {
        let (payloads, _) = World::with_hero(Hero {
            nested_object: None,
            ..Hero::luke()
        })
        .run(
            r#"
      query {
        hero {
          nestedObject {
            name
          }
          ... @defer {
            nestedObject {
              name
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
            &[json!({"data": {"hero": {"nestedObject": null}}})],
        );
    }

    /// it('Deduplicates promise object fields')
    #[test]
    fn deduplicates_promise_object_fields() {
        let (payloads, _) = World {
            slow: vec!["Hero.nestedObject"],
            ..World::with_hero(Hero {
                nested_object: Some(NestedObject {
                    name: Ok(Some("foo".into())),
                    ..Default::default()
                }),
                ..Hero::luke()
            })
        }
        .run(
            r#"
      query {
        hero {
          nestedObject {
            name
          }
          ... @defer {
            nestedObject {
              name
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
            &[json!({"data": {"hero": {"nestedObject": {"name": "foo"}}}})],
        );
    }

    /// it('Handles errors thrown in deferred fragments')
    #[test]
    fn handles_errors_thrown_in_deferred_fragments() {
        let (payloads, _) = World::with_hero(Hero {
            name: Err("bad"),
            ..Hero::luke()
        })
        .run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
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
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [{
                        "id": "0",
                        "data": {"name": null},
                        "errors": [{
                            "message": "bad",
                            "locations": [{"line": 9, "column": 9}],
                            "path": ["hero", "name"],
                        }],
                    }],
                    "completed": [{"id": "0"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles non-nullable errors thrown in deferred fragments')
    #[test]
    fn handles_non_nullable_errors_thrown_in_deferred_fragments() {
        let (payloads, _) = World::with_hero(Hero {
            non_null_name: Err(HERO_NON_NULL_NAME),
            ..Hero::luke()
        })
        .run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
        nonNullName
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": HERO_NON_NULL_NAME,
                            "locations": [{"line": 9, "column": 9}],
                            "path": ["hero", "nonNullName"],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Handles non-nullable errors thrown outside deferred fragments')
    #[test]
    fn handles_non_nullable_errors_thrown_outside_deferred_fragments() {
        let (payloads, _) = World::with_hero(Hero {
            non_null_name: Err(HERO_NON_NULL_NAME),
            ..Hero::luke()
        })
        .run(
            r#"
      query HeroNameQuery {
        hero {
          nonNullName
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
        id
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "data": {"hero": null},
                "errors": [{
                    "message": HERO_NON_NULL_NAME,
                    "locations": [{"line": 4, "column": 11}],
                    "path": ["hero", "nonNullName"],
                }],
            })],
        );
    }

    /// it('Handles async non-nullable errors thrown in deferred fragments')
    #[test]
    fn handles_async_non_nullable_errors_thrown_in_deferred_fragments() {
        let (payloads, _) = World {
            slow: vec!["Hero.nonNullName"],
            ..World::with_hero(Hero {
                non_null_name: Err(HERO_NON_NULL_NAME),
                ..Hero::luke()
            })
        }
        .run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
        nonNullName
      }
    "#,
            Value::Null,
            incremental(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "completed": [{
                        "id": "0",
                        "errors": [{
                            "message": HERO_NON_NULL_NAME,
                            "locations": [{"line": 9, "column": 9}],
                            "path": ["hero", "nonNullName"],
                        }],
                    }],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Returns payloads in correct order')
    #[test]
    fn returns_payloads_in_correct_order() {
        let (payloads, _) = World {
            slow: vec!["Hero.name"],
            ..World::with_hero(Hero {
                name: Ok(Some("slow".into())),
                ..Hero::luke()
            })
        }
        .run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
        name
        friends {
          ...NestedFragment @defer
        }
      }
      fragment NestedFragment on Friend {
        name
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: the friends' fragments are announced and delivered in the
        // same payload as NameFragment; greem releases them after it ships.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [
                        {"id": "1", "path": ["hero", "friends", 0]},
                        {"id": "2", "path": ["hero", "friends", 1]},
                        {"id": "3", "path": ["hero", "friends", 2]},
                    ],
                    "incremental": [{"id": "0", "data": {"name": "slow", "friends": [{}, {}, {}]}}],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "1", "data": {"name": "Han"}},
                        {"id": "2", "data": {"name": "Leia"}},
                        {"id": "3", "data": {"name": "C-3PO"}},
                    ],
                    "completed": [{"id": "1"}, {"id": "2"}, {"id": "3"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Returns payloads from synchronous data in correct order')
    #[test]
    fn returns_payloads_from_synchronous_data_in_correct_order() {
        let (payloads, _) = World::with_hero(Hero::luke()).run(
            r#"
      query HeroNameQuery {
        hero {
          id
          ...NameFragment @defer
        }
      }
      fragment NameFragment on Hero {
        name
        friends {
          ...NestedFragment @defer
        }
      }
      fragment NestedFragment on Friend {
        name
      }
    "#,
            Value::Null,
            incremental(),
        );
        // upstream: two payloads, as in `returns_payloads_in_correct_order`.
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {"hero": {"id": "1"}},
                    "pending": [{"id": "0", "path": ["hero"]}],
                    "hasNext": true,
                }),
                json!({
                    "pending": [
                        {"id": "1", "path": ["hero", "friends", 0]},
                        {"id": "2", "path": ["hero", "friends", 1]},
                        {"id": "3", "path": ["hero", "friends", 2]},
                    ],
                    "incremental": [{"id": "0", "data": {"name": "Luke", "friends": [{}, {}, {}]}}],
                    "completed": [{"id": "0"}],
                    "hasNext": true,
                }),
                json!({
                    "incremental": [
                        {"id": "1", "data": {"name": "Han"}},
                        {"id": "2", "data": {"name": "Leia"}},
                        {"id": "3", "data": {"name": "C-3PO"}},
                    ],
                    "completed": [{"id": "1"}, {"id": "2"}, {"id": "3"}],
                    "hasNext": false,
                }),
            ],
        );
    }

    /// it('Filters deferred payloads when a list item returned by an async iterable is nulled')
    #[test]
    fn filters_deferred_payloads_when_a_list_item_returned_by_an_async_iterable_is_nulled() {
        let (payloads, _) = World {
            slow: vec!["Friend.nonNullName"],
            ..World::with_hero(Hero {
                friends: Some(vec![Friend {
                    non_null_name: Err(FRIEND_NON_NULL_NAME),
                    ..Friend::new("2", "Han")
                }]),
                friends_async: true,
                ..Hero::luke()
            })
        }
        .run(
            r#"
      query {
        hero {
          friends {
            nonNullName
            ...NameFragment @defer
          }
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
            &[json!({
                "data": {"hero": {"friends": [null]}},
                "errors": [{
                    "message": FRIEND_NON_NULL_NAME,
                    "locations": [{"line": 5, "column": 13}],
                    "path": ["hero", "friends", 0, "nonNullName"],
                }],
            })],
        );
    }

    // it('should allow deferred execution when passed abortSignal, if not aborted')
    // Not ported, reason (ii): the subject is the abort signal plumbing.

    // it('should stop deferred execution when aborted')
    // Not ported, reason (ii): aborting a request through an abort signal.

    // it('should stop deferred execution when aborted mid-execution')
    // Not ported, reason (ii): aborting a request through an abort signal.

    // it('cancels pending deferred execution groups')
    // Not ported, reason (ii): aborting a request through an abort signal.

    // it('cancels pending deferred tasks with async child stream cleanup')
    // Not ported, reason (ii): an abort signal and the async iterator's `return()`.

    // it('should ignore deferred payloads resolved after cancellation')
    // Not ported, reason (ii): promise timing after an abort signal.

    // it('should ignore deferred errors after cancellation')
    // Not ported, reason (ii): promise timing after an abort signal.
}
