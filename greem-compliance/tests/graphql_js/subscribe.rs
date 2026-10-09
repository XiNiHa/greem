//! graphql-js v17.0.2 `src/execution/__tests__/subscribe-test.ts`, case by case in upstream order.
//!
//! greem executes no subscription yet (#40): the cases that subscribe are
//! written against the world's pre-seeded source and ignored; the cases
//! rejected at parse pass today.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::subscribe::{Email, Event, World};
use greem_compliance::harness::{Area, Harness};
use serde_json::{Value, json};

/// Upstream's `fooGenerator`: one `foo` event.
fn foo_world() -> World {
    World {
        events: vec![Event::Foo("FooValue")],
        ..Default::default()
    }
}

/// describe('Subscription Initialization Phase')
mod subscription_initialization_phase {
    use super::*;

    // it('throws for legacy ExecutionArgs passed to createSourceEventStream')
    // Not ported, reason (ii): the argument shape of a JS API entry point.

    // it('throws when validateSubscriptionArgs is called with a non-subscription operation')
    // Not ported, reason (ii): greem has one `execute` for every operation kind; there is no subscribe entry point to hand a query.

    // it('throws when subscribe is called with a non-subscription operation')
    // Not ported, reason (ii): same as above.

    /// it('accepts multiple subscription fields defined in schema')
    #[test]
    #[ignore = "#40"]
    fn accepts_multiple_subscription_fields_defined_in_schema() {
        let (payloads, _) = foo_world().run(
            "subscription { foo }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(&payloads, &[json!({"data": {"foo": "FooValue"}})]);
    }

    /// it('accepts type definition with sync subscribe function')
    #[test]
    #[ignore = "#40"]
    fn accepts_type_definition_with_sync_subscribe_function() {
        let (payloads, _) = foo_world().run(
            "subscription { foo }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(&payloads, &[json!({"data": {"foo": "FooValue"}})]);
    }

    /// it('accepts type definition with async subscribe function')
    ///
    /// The source is created after one yield.
    #[test]
    #[ignore = "#40"]
    fn accepts_type_definition_with_async_subscribe_function() {
        let world = World {
            harness: Harness {
                yields: vec![1],
                ..Default::default()
            },
            ..foo_world()
        };
        let (payloads, _) = world.run(
            "subscription { foo }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(&payloads, &[json!({"data": {"foo": "FooValue"}})]);
    }

    // it('uses a custom default subscribeFieldResolver')
    // Not ported, reason (iii): `subscribeFieldResolver` is a graphql-js option greem lacks.

    // it('maps a source stream to response events with a custom rootSelectionSetExecutor')
    // Not ported, reason (ii): `mapSourceToResponseEvent` with a custom executor is JS API plumbing.

    /// it('should only resolve the first field of invalid multi-field')
    ///
    /// Upstream executes without validating and asserts which resolver ran
    /// (reason (i) on its own); greem validates at parse, so the second
    /// root field is a request error.
    #[test]
    fn should_only_resolve_the_first_field_of_invalid_multi_field() {
        let (v, _) = foo_world().single(
            "subscription { foo bar }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "anonymous subscription can only have one root field",
                    "locations": [{"line": 1, "column": 1}],
                }],
            }),
        );
    }

    /// it('resolves to an error if schema does not support subscriptions')
    ///
    /// Runs against the mutations area, whose schema declares no
    /// subscription root, as upstream's `DummyQueryType` schema does not.
    #[test]
    fn resolves_to_an_error_if_schema_does_not_support_subscriptions() {
        let (v, _) = greem_compliance::graphql_js::mutations::World::new(0).single(
            "subscription { unknownField }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "`subscription` root operation type is not defined",
                    "locations": [{"line": 1, "column": 1}],
                }],
            }),
        );
    }

    /// it('resolves to an error for unknown subscription field')
    #[test]
    fn resolves_to_an_error_for_unknown_subscription_field() {
        let (v, _) = foo_world().single(
            "subscription { unknownField }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "errors": [{
                    "message": "type `Subscription` does not have a field `unknownField`",
                    "locations": [{"line": 1, "column": 16}],
                }],
            }),
        );
    }

    // it('should pass through unexpected errors thrown in subscribe')
    // Not ported, reason (ii): passes `{}` as the document; greem's document is a parsed value.

    // it('throws an error if subscribe does not return an iterator')
    // Not ported, reason (iv): the source's type is fixed by the resolver's signature.

    /// it('resolves to an error for subscription resolver errors')
    ///
    /// Upstream's "returning an error" and "throwing an error": the source
    /// fails before any event, so the result is one error response with no
    /// data, the error at the root field. greem's shape for a failed
    /// source is undecided (#40); upstream's stands in.
    #[test]
    #[ignore = "#40"]
    fn resolves_to_an_error_for_subscription_resolver_errors() {
        let world = World {
            source_error: Some("test error"),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "subscription { foo }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "test error",
                    "locations": [{"line": 1, "column": 16}],
                    "path": ["foo"],
                }],
            })],
        );
    }

    /// it('resolves to an error for subscription resolver errors')
    ///
    /// Upstream's "resolving to an error" and "rejecting with an error":
    /// the source fails after one yield.
    #[test]
    #[ignore = "#40"]
    fn resolves_to_an_error_for_subscription_resolver_errors_async() {
        let world = World {
            harness: Harness {
                yields: vec![1],
                ..Default::default()
            },
            source_error: Some("test error"),
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "subscription { foo }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "test error",
                    "locations": [{"line": 1, "column": 16}],
                    "path": ["foo"],
                }],
            })],
        );
    }

    /// it('resolves to an error if variables were wrong type')
    ///
    /// Today the subscription is rejected before its variables are coerced.
    /// The coercion wording was learnt by running the same variable against
    /// a query operation; greem's coercion error carries no location.
    #[test]
    #[ignore = "#40"]
    fn resolves_to_an_error_if_variables_were_wrong_type() {
        let (payloads, _) = foo_world().run(
            r#"
      subscription ($arg: Int) {
        foo(arg: $arg)
      }
    "#,
            json!({"arg": "meow"}),
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "errors": [{
                    "message": "could not coerce variable arg: \"meow\" to type Int",
                }],
            })],
        );
    }
}

/// describe('Subscription Publish Phase')
mod subscription_publish_phase {
    use super::*;

    /// Upstream's `createSubscription` document. Upstream selects `emails`,
    /// an object list, without a subselection, which graphql-js tolerates
    /// because `subscribe` does not validate; greem validates at parse
    /// ("interface, union and object types must have a subselection set"),
    /// so `emails` selects `{ from }` here. Every location is upstream's.
    const SUBSCRIPTION: &str = r#"
    subscription (
      $priority: Int = 0
      $shouldDefer: Boolean = false
      $shouldStream: Boolean = false
      $asyncResolver: Boolean = false
    ) {
      importantEmail(priority: $priority) {
        email {
          from
          subject
          ... @include(if: $asyncResolver) {
            asyncSubject
          }
        }
        ... @defer(if: $shouldDefer) {
          inbox {
            emails @include(if: $shouldStream) @stream(if: $shouldStream) { from }
            unread
            total
          }
        }
      }
    }
  "#;

    fn yuzhi() -> Email {
        Email {
            from: "yuzhi@graphql.org",
            subject: "Alright",
            message: "Tests are good",
            unread: true,
        }
    }

    fn hyo() -> Email {
        Email {
            from: "hyo@graphql.org",
            subject: "Tools",
            message: "I <3 making things",
            unread: true,
        }
    }

    // it('produces a payload for multiple subscribe in same subscription')
    // Not ported, reason (ii): the pubsub's fan-out to two subscribers is the subject.

    /// it('produces a payload when queried fields are async')
    #[test]
    #[ignore = "#40"]
    fn produces_a_payload_when_queried_fields_are_async() {
        let (payloads, _) = World::email_world(vec![yuzhi()]).run(
            SUBSCRIPTION,
            json!({"asyncResolver": true}),
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[json!({
                "data": {
                    "importantEmail": {
                        "email": {
                            "from": "yuzhi@graphql.org",
                            "subject": "Alright",
                            "asyncSubject": "Alright",
                        },
                        "inbox": {"unread": 1, "total": 2},
                    },
                },
            })],
        );
    }

    /// it('produces a payload per subscription event')
    ///
    /// The two events; the `return()` that follows them, the emit it
    /// refuses and the completed `next()` are the iterator protocol.
    #[test]
    #[ignore = "#40"]
    fn produces_a_payload_per_subscription_event() {
        let (payloads, _) = World::email_world(vec![yuzhi(), hyo()]).run(
            SUBSCRIPTION,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "importantEmail": {
                            "email": {"from": "yuzhi@graphql.org", "subject": "Alright"},
                            "inbox": {"unread": 1, "total": 2},
                        },
                    },
                }),
                json!({
                    "data": {
                        "importantEmail": {
                            "email": {"from": "hyo@graphql.org", "subject": "Tools"},
                            "inbox": {"unread": 2, "total": 3},
                        },
                    },
                }),
            ],
        );
    }

    /// it('subscribe function returns errors with @defer')
    ///
    /// `@defer` with a variable `if` passes validation and is an execution
    /// error at the root field once the variable is true, for every event.
    /// greem's wording for it is undecided (#40); upstream's stands in.
    #[test]
    #[ignore = "#40"]
    fn subscribe_function_returns_errors_with_defer() {
        let (payloads, _) = World::email_world(vec![yuzhi(), hyo()]).run(
            SUBSCRIPTION,
            json!({"shouldDefer": true}),
            incremental(),
        );
        let error_payload = json!({
            "data": {"importantEmail": null},
            "errors": [{
                "message": "`@defer` directive not supported on subscription operations. Disable `@defer` by setting the `if` argument to `false`.",
                "locations": [{"line": 8, "column": 7}],
                "path": ["importantEmail"],
            }],
        });
        assert_payloads(&payloads, &[error_payload.clone(), error_payload]);
    }

    /// it('subscribe function returns errors with @stream')
    ///
    /// As above at the streamed field: the error nulls `emails` and the
    /// rest of the event completes.
    #[test]
    #[ignore = "#40"]
    fn subscribe_function_returns_errors_with_stream() {
        let (payloads, _) = World::email_world(vec![yuzhi(), hyo()]).run(
            SUBSCRIPTION,
            json!({"shouldStream": true}),
            incremental(),
        );
        let error = json!({
            "message": "`@stream` directive not supported on subscription operations. Disable `@stream` by setting the `if` argument to `false`.",
            "locations": [{"line": 18, "column": 13}],
            "path": ["importantEmail", "inbox", "emails"],
        });
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "importantEmail": {
                            "email": {"from": "yuzhi@graphql.org", "subject": "Alright"},
                            "inbox": {"emails": null, "unread": 1, "total": 2},
                        },
                    },
                    "errors": [error],
                }),
                json!({
                    "data": {
                        "importantEmail": {
                            "email": {"from": "hyo@graphql.org", "subject": "Tools"},
                            "inbox": {"emails": null, "unread": 2, "total": 3},
                        },
                    },
                    "errors": [error],
                }),
            ],
        );
    }

    /// it('produces a payload when there are multiple events')
    #[test]
    #[ignore = "#40"]
    fn produces_a_payload_when_there_are_multiple_events() {
        let second = Email {
            from: "yuzhi@graphql.org",
            subject: "Alright 2",
            message: "Tests are good 2",
            unread: true,
        };
        let (payloads, _) = World::email_world(vec![yuzhi(), second]).run(
            SUBSCRIPTION,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({
                    "data": {
                        "importantEmail": {
                            "email": {"from": "yuzhi@graphql.org", "subject": "Alright"},
                            "inbox": {"unread": 1, "total": 2},
                        },
                    },
                }),
                json!({
                    "data": {
                        "importantEmail": {
                            "email": {"from": "yuzhi@graphql.org", "subject": "Alright 2"},
                            "inbox": {"unread": 2, "total": 3},
                        },
                    },
                }),
            ],
        );
    }

    // it('should not trigger when subscription is already done')
    // Not ported, reason (ii): the iterator's `return()` is the subject.

    // it('should not trigger when subscription is thrown')
    // Not ported, reason (ii): the iterator's `throw()` is the subject.

    // it('event order is correct for multiple publishes')
    // Not ported, reason (ii): the inbox totals come from the pubsub subscriber running at emit time, before either event executes; a pre-seeded source has no emit time.

    /// it('should handle error during execution of source event')
    ///
    /// An event's execution error is one response; the stream goes on.
    #[test]
    #[ignore = "#40"]
    fn should_handle_error_during_execution_of_source_event() {
        let world = World {
            events: vec![
                Event::NewMessage("Hello"),
                Event::NewMessage("Goodbye"),
                Event::NewMessage("Bonjour"),
            ],
            ..Default::default()
        };
        let (payloads, _) = world.run(
            "subscription { newMessage }",
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_payloads(
            &payloads,
            &[
                json!({"data": {"newMessage": "Hello"}}),
                json!({
                    "data": {"newMessage": null},
                    "errors": [{
                        "message": "Never leave.",
                        "locations": [{"line": 1, "column": 16}],
                        "path": ["newMessage"],
                    }],
                }),
                json!({"data": {"newMessage": "Bonjour"}}),
            ],
        );
    }

    // it('should pass through error thrown in source event stream')
    // Not ported, reason (ii): asserts the iterator rejecting with the raw source error, not a response; the payload before it is covered above.
}
