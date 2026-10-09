//! graphql-js v17.0.2 `src/execution/__tests__/union-interface-test.ts`, case by case in upstream order.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::union_interface::{PersonData, Ref, World};
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// describe('Execute: Union and intersection types')
mod union_and_intersection_types {
    use super::*;

    /// it('can introspect on union and intersection types')
    ///
    /// `possibleTypes` follow the schema's declaration order, which the area
    /// schema keeps as upstream lists them.
    #[test]
    fn can_introspect_on_union_and_intersection_types() {
        let (v, _) = World::upstream().single(
            r#"
      {
        Named: __type(name: "Named") {
          kind
          name
          fields { name }
          interfaces { name }
          possibleTypes { name }
          enumValues { name }
          inputFields { name }
        }
        Mammal: __type(name: "Mammal") {
          kind
          name
          fields { name }
          interfaces { name }
          possibleTypes { name }
          enumValues { name }
          inputFields { name }
        }
        Pet: __type(name: "Pet") {
          kind
          name
          fields { name }
          interfaces { name }
          possibleTypes { name }
          enumValues { name }
          inputFields { name }
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
                    "Named": {
                        "kind": "INTERFACE",
                        "name": "Named",
                        "fields": [{"name": "name"}],
                        "interfaces": [],
                        "possibleTypes": [
                            {"name": "Dog"},
                            {"name": "Cat"},
                            {"name": "Person"},
                            {"name": "Plant"},
                        ],
                        "enumValues": null,
                        "inputFields": null,
                    },
                    "Mammal": {
                        "kind": "INTERFACE",
                        "name": "Mammal",
                        "fields": [{"name": "progeny"}, {"name": "mother"}, {"name": "father"}],
                        "interfaces": [{"name": "Life"}],
                        "possibleTypes": [{"name": "Dog"}, {"name": "Cat"}, {"name": "Person"}],
                        "enumValues": null,
                        "inputFields": null,
                    },
                    "Pet": {
                        "kind": "UNION",
                        "name": "Pet",
                        "fields": null,
                        "interfaces": null,
                        "possibleTypes": [{"name": "Dog"}, {"name": "Cat"}],
                        "enumValues": null,
                        "inputFields": null,
                    },
                },
            }),
        );
    }

    /// it('executes using union types')
    ///
    /// Upstream executes this invalid query without validating; greem
    /// validates at parse, so the union's fields are request errors. The
    /// wording is apollo-compiler's, learnt by running the request.
    #[test]
    fn executes_using_union_types() {
        let (v, _) = World::upstream().single(
            r#"
      {
        __typename
        name
        pets {
          __typename
          name
          barks
          meows
        }
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
                        "message": "type `Pet` does not have a field `name`",
                        "locations": [{"line": 7, "column": 11}],
                    },
                    {
                        "message": "type `Pet` does not have a field `barks`",
                        "locations": [{"line": 8, "column": 11}],
                    },
                    {
                        "message": "type `Pet` does not have a field `meows`",
                        "locations": [{"line": 9, "column": 11}],
                    },
                ],
            }),
        );
    }

    /// it('executes union types with inline fragments')
    #[test]
    fn executes_union_types_with_inline_fragments() {
        let (v, _) = World::upstream().single(
            r#"
      {
        __typename
        name
        pets {
          __typename
          ... on Dog {
            name
            barks
          }
          ... on Cat {
            name
            meows
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
                    "__typename": "Person",
                    "name": "John",
                    "pets": [
                        {"__typename": "Cat", "name": "Garfield", "meows": false},
                        {"__typename": "Dog", "name": "Odie", "barks": true},
                    ],
                },
            }),
        );
    }

    /// it('executes using interface types')
    ///
    /// Upstream executes this invalid query without validating; greem
    /// validates at parse, so the fields the interface lacks are request
    /// errors. The wording is apollo-compiler's, learnt by running the request.
    #[test]
    fn executes_using_interface_types() {
        let (v, _) = World::upstream().single(
            r#"
      {
        __typename
        name
        friends {
          __typename
          name
          barks
          meows
        }
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
                        "message": "type `Named` does not have a field `barks`",
                        "locations": [{"line": 8, "column": 11}],
                    },
                    {
                        "message": "type `Named` does not have a field `meows`",
                        "locations": [{"line": 9, "column": 11}],
                    },
                ],
            }),
        );
    }

    /// it('executes interface types with inline fragments')
    #[test]
    fn executes_interface_types_with_inline_fragments() {
        let (v, _) = World::upstream().single(
            r#"
      {
        __typename
        name
        friends {
          __typename
          name
          ... on Dog {
            barks
          }
          ... on Cat {
            meows
          }

          ... on Mammal {
            mother {
              __typename
              ... on Dog {
                name
                barks
              }
              ... on Cat {
                name
                meows
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
                    "__typename": "Person",
                    "name": "John",
                    "friends": [
                        {"__typename": "Person", "name": "Liz", "mother": null},
                        {
                            "__typename": "Dog",
                            "name": "Odie",
                            "barks": true,
                            "mother": {"__typename": "Dog", "name": "Odie's Mom", "barks": true},
                        },
                    ],
                },
            }),
        );
    }

    /// it('executes interface types with named fragments')
    #[test]
    fn executes_interface_types_with_named_fragments() {
        let (v, _) = World::upstream().single(
            r#"
      {
        __typename
        name
        friends {
          __typename
          name
          ...DogBarks
          ...CatMeows
        }
      }

      fragment  DogBarks on Dog {
        barks
      }

      fragment  CatMeows on Cat {
        meows
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "__typename": "Person",
                    "name": "John",
                    "friends": [
                        {"__typename": "Person", "name": "Liz"},
                        {"__typename": "Dog", "name": "Odie", "barks": true},
                    ],
                },
            }),
        );
    }

    /// it('allows fragment conditions to be abstract types')
    #[test]
    fn allows_fragment_conditions_to_be_abstract_types() {
        let (v, _) = World::upstream().single(
            r#"
      {
        __typename
        name
        pets {
          ...PetFields,
          ...on Mammal {
            mother {
              ...ProgenyFields
            }
          }
        }
        friends { ...FriendFields }
      }

      fragment PetFields on Pet {
        __typename
        ... on Dog {
          name
          barks
        }
        ... on Cat {
          name
          meows
        }
      }

      fragment FriendFields on Named {
        __typename
        name
        ... on Dog {
          barks
        }
        ... on Cat {
          meows
        }
      }

      fragment ProgenyFields on Life {
        progeny {
          __typename
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
                    "__typename": "Person",
                    "name": "John",
                    "pets": [
                        {
                            "__typename": "Cat",
                            "name": "Garfield",
                            "meows": false,
                            "mother": {"progeny": [{"__typename": "Cat"}]},
                        },
                        {
                            "__typename": "Dog",
                            "name": "Odie",
                            "barks": true,
                            "mother": {"progeny": [{"__typename": "Dog"}]},
                        },
                    ],
                    "friends": [
                        {"__typename": "Person", "name": "Liz"},
                        {"__typename": "Dog", "name": "Odie", "barks": true},
                    ],
                },
            }),
        );
    }

    // it('gets execution info in resolver')
    // Not ported, reason (i): asserts the `info`, context and root value handed to `resolveType`, which greem has no counterpart of.

    /// it('it handles rejections from isTypeOf after after an isTypeOf returns true')
    ///
    /// Upstream's `Plant.isTypeOf` rejects after `Cat.isTypeOf` matched;
    /// greem partitions on the member wrapper, so the claim left is that the
    /// cat completes as a `Cat`.
    #[test]
    fn it_handles_rejections_from_is_type_of_after_after_an_is_type_of_returns_true() {
        let mut world = World::upstream();
        // Upstream's `new Person('John', [], [liz], [garfield])`.
        world.people[0] = PersonData {
            name: "John",
            pets: Some(vec![]),
            friends: Some(vec![Ref::Person(1)]),
            responsibilities: Some(vec![Ref::Cat(0)]),
            progeny: vec![],
            mother: None,
            father: None,
        };
        let (v, _) = world.single(
            r#"
      {
        responsibilities {
          __typename
          ... on Dog {
            name
            barks
          }
          ... on Cat {
            name
            meows
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
                    "responsibilities": [
                        {"__typename": "Cat", "name": "Garfield", "meows": false},
                    ],
                },
            }),
        );
    }

    /// it('handles promises from isTypeOf correctly when a later type matches synchronously')
    ///
    /// Upstream's `TypeA.isTypeOf` rejects later while `TypeB.isTypeOf`
    /// matches; greem partitions on the member wrapper, so the claim left is
    /// that `search(id: "b")` completes as a `TypeB`.
    #[test]
    fn handles_promises_from_is_type_of_correctly_when_a_later_type_matches_synchronously() {
        let (v, _) = World::upstream().single(
            r#"
      query TestSearch {
        search(id: "b") {
          __typename
          id
          ... on TypeA {
            nameA
          }
          ... on TypeB {
            nameB
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
                    "search": {"__typename": "TypeB", "id": "b", "nameB": "Object B"},
                },
            }),
        );
    }

    // it('handles pending isTypeOf rejections when a later isTypeOf throws synchronously')
    // Not ported, reason (ii): the asserted response is the synchronous `isTypeOf` throw and the pending rejection it must not leak; greem has no type-resolution step to throw from.
}
