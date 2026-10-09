//! graphql-js v17.0.2 `src/execution/__tests__/abstract-test.ts`, case by case in upstream order.
//!
//! Upstream runs every query twice, with sync and with promise-returning
//! `isTypeOf`/`resolveType`, and expects the same response; greem has no
//! type-resolution step, so each case runs once.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::r#abstract::{Cat, Dog, Pet, World};
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// Upstream's `[new Dog('Odie', true), new Cat('Garfield', false)]`.
fn odie_and_garfield() -> World {
    World {
        pets: vec![
            Pet::Dog(Dog {
                name: "Odie",
                woofs: true,
            }),
            Pet::Cat(Cat {
                name: "Garfield",
                meows: false,
            }),
        ],
        ..Default::default()
    }
}

/// describe('Execute: Handles execution of abstract types')
mod handles_execution_of_abstract_types {
    use super::*;

    /// it('isTypeOf used to resolve runtime type for Interface')
    #[test]
    fn is_type_of_used_to_resolve_runtime_type_for_interface() {
        let (v, _) = odie_and_garfield().single(
            r#"
      {
        pets {
          name
          ... on Dog {
            woofs
          }
          ... on Cat {
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
                    "pets": [
                        {"name": "Odie", "woofs": true},
                        {"name": "Garfield", "meows": false},
                    ],
                },
            }),
        );
    }

    /// it('isTypeOf can throw')
    ///
    /// The throw happens while completing each pet, so each item fails in
    /// place with the error at its index.
    #[test]
    fn is_type_of_can_throw() {
        let (v, _) = World {
            pet_error: Some("We are testing this error"),
            ..odie_and_garfield()
        }
        .single(
            r#"
      {
        pets {
          name
          ... on Dog {
            woofs
          }
          ... on Cat {
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
                    "pets": [null, null],
                },
                "errors": [
                    {
                        "message": "We are testing this error",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["pets", 0],
                    },
                    {
                        "message": "We are testing this error",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["pets", 1],
                    },
                ],
            }),
        );
    }

    // it('isTypeOf can return false')
    // Not ported, reason (iv): a value at an abstract position always carries its member type, so no pet can fail to resolve to an object type.

    /// it('isTypeOf used to resolve runtime type for Union')
    #[test]
    fn is_type_of_used_to_resolve_runtime_type_for_union() {
        let (v, _) = odie_and_garfield().single(
            r#"{
      unionPets {
        ... on Dog {
          name
          woofs
        }
        ... on Cat {
          name
          meows
        }
      }
    }"#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "unionPets": [
                        {"name": "Odie", "woofs": true},
                        {"name": "Garfield", "meows": false},
                    ],
                },
            }),
        );
    }

    /// it('resolveType can throw')
    #[test]
    fn resolve_type_can_throw() {
        let (v, _) = World {
            pet_error: Some("We are testing this error"),
            ..odie_and_garfield()
        }
        .single(
            r#"
      {
        pets {
          name
          ... on Dog {
            woofs
          }
          ... on Cat {
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
                    "pets": [null, null],
                },
                "errors": [
                    {
                        "message": "We are testing this error",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["pets", 0],
                    },
                    {
                        "message": "We are testing this error",
                        "locations": [{"line": 3, "column": 9}],
                        "path": ["pets", 1],
                    },
                ],
            }),
        );
    }

    /// it('resolve Union type using __typename on source object')
    ///
    /// Upstream selects `name` directly on the union and executes without
    /// validating; greem validates at parse, so that selection is a request
    /// error. The wording is apollo-compiler's, learnt by running the request.
    #[test]
    fn resolve_union_type_using_typename_on_source_object() {
        let (v, _) = odie_and_garfield().single(
            r#"
      {
        unionPets {
          name
          ... on Dog {
            woofs
          }
          ... on Cat {
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
                "errors": [{
                    "message": "type `PetUnion` does not have a field `name`",
                    "locations": [{"line": 4, "column": 11}],
                }],
            }),
        );
    }

    /// it('resolve Interface type using __typename on source object')
    #[test]
    fn resolve_interface_type_using_typename_on_source_object() {
        let (v, _) = odie_and_garfield().single(
            r#"
      {
        pets {
          name
          ... on Dog {
            woofs
          }
          ... on Cat {
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
                    "pets": [
                        {"name": "Odie", "woofs": true},
                        {"name": "Garfield", "meows": false},
                    ],
                },
            }),
        );
    }

    // it('resolveType on Interface yields useful error')
    // Not ported, reason (iv): every asserted message is a missing, unknown, non-object or impossible runtime type, which the member wrapper rules out at compile time.
}
