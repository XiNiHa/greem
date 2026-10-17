//! `greem::http::handle` end to end over `greem-test-app`: methods, media
//! types and status codes as the GraphQL-over-HTTP draft sets them.

use futures::StreamExt;
use futures::executor::block_on;
use greem::http::{Body, RequestParts};
use greem::{ExecuteOptions, IncrementalDelivery, Roots};
use greem_test_app::app::{App, MutationRoot, QueryRoot, SubscriptionRoot, build_schema};
use serde_json::{Value, json};

const GRAPHQL_RESPONSE_JSON: &str = "application/graphql-response+json; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";

struct Sent {
    status: u16,
    content_type: &'static str,
    allow: Option<&'static str>,
    body: String,
}

impl Sent {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.body))
    }
}

fn send(app: App, request: RequestParts<'_>) -> Sent {
    let options = ExecuteOptions {
        incremental: IncrementalDelivery::Enabled,
        ..Default::default()
    };
    send_with(app, request, options)
}

fn send_with(app: App, request: RequestParts<'_>, options: ExecuteOptions) -> Sent {
    let schema = build_schema();
    let roots = Roots::query(QueryRoot)
        .with_mutation(MutationRoot)
        .with_subscription(SubscriptionRoot);
    let response = block_on(greem::http::handle(&schema, roots, app, request, options));
    let body = match response.body {
        Body::Full(bytes) => bytes,
        Body::Parts(parts) => block_on(parts.concat()),
    };
    Sent {
        status: response.status,
        content_type: response.content_type,
        allow: response.allow,
        body: String::from_utf8(body).unwrap(),
    }
}

fn post<'a>(accept: Option<&'a str>, body: &'a str) -> RequestParts<'a> {
    RequestParts {
        method: "POST",
        content_type: Some("application/json"),
        accept: accept.map(Into::into),
        body: body.as_bytes(),
        ..Default::default()
    }
}

fn get(query_string: &str) -> RequestParts<'_> {
    RequestParts {
        method: "GET",
        query_string: Some(query_string),
        accept: Some("application/graphql-response+json".into()),
        ..Default::default()
    }
}

const GRAPHQL: Option<&str> = Some("application/graphql-response+json");
const LEGACY: Option<&str> = Some("application/json");

#[test]
fn a_post_answers_in_the_negotiated_media_type() {
    let body = r#"{"query": "{ users { name } }"}"#;
    for (accept, content_type) in [
        (GRAPHQL, GRAPHQL_RESPONSE_JSON),
        (LEGACY, JSON),
        (None, JSON),
        (Some("*/*"), JSON),
    ] {
        let sent = send(App::default(), post(accept, body));
        assert_eq!(sent.status, 200, "{accept:?}");
        assert_eq!(sent.content_type, content_type, "{accept:?}");
        assert_eq!(
            sent.json(),
            json!({"data": {"users": [{"name": "Ann"}, {"name": "Bob"}]}})
        );
    }
}

#[test]
fn a_get_reads_the_query_string() {
    let sent = send(
        App::default(),
        get(
            "query=query+User(%24id%3A+ID!)+%7B+user(id%3A+%24id)+%7B+name+%7D+%7D&variables=%7B%22id%22%3A%222%22%7D",
        ),
    );
    assert_eq!(sent.status, 200);
    assert_eq!(sent.json(), json!({"data": {"user": {"name": "Bob"}}}));
}

#[test]
fn a_query_request_reads_its_body_like_a_post() {
    let body =
        r#"{"query": "query User($id: ID!) { user(id: $id) { name } }", "variables": {"id": "2"}}"#;
    let sent = send(
        App::default(),
        RequestParts {
            method: "QUERY",
            ..post(GRAPHQL, body)
        },
    );
    assert_eq!(sent.status, 200);
    assert_eq!(sent.content_type, GRAPHQL_RESPONSE_JSON);
    assert_eq!(sent.json(), json!({"data": {"user": {"name": "Bob"}}}));

    let sent = send(
        App::default(),
        RequestParts {
            method: "QUERY",
            content_type: None,
            ..post(GRAPHQL, body)
        },
    );
    assert_eq!(sent.status, 415);
}

#[test]
fn a_mutation_needs_a_post() {
    let sent = send(
        App::default(),
        get("query=mutation+%7B+rename(id%3A+%221%22%2C+name%3A+%22Z%22)+%7B+name+%7D+%7D"),
    );
    assert_eq!(sent.status, 405);
    assert_eq!(sent.allow, Some("POST"));
    assert_eq!(sent.content_type, JSON);

    let mutation = post(
        GRAPHQL,
        r#"{"query": "mutation { rename(id: \"1\", name: \"Z\") { name } }"}"#,
    );
    let sent = send(
        App::default(),
        RequestParts {
            method: "QUERY",
            ..mutation.clone()
        },
    );
    assert_eq!(sent.status, 422);
    assert_eq!(sent.allow, None);
    assert_eq!(sent.content_type, JSON);

    let sent = send(App::default(), mutation);
    assert_eq!(sent.status, 200);
    assert_eq!(sent.json(), json!({"data": {"rename": {"name": "Z"}}}));
}

#[test]
fn other_methods_are_not_allowed() {
    for method in ["PUT", "query"] {
        let sent = send(
            App::default(),
            RequestParts {
                method,
                ..post(GRAPHQL, r#"{"query": "{ users { name } }"}"#)
            },
        );
        assert_eq!(sent.status, 405, "{method}");
        assert_eq!(sent.allow, Some("GET, POST, QUERY"), "{method}");
    }
}

#[test]
fn an_unacceptable_accept_is_406() {
    let sent = send(
        App::default(),
        post(Some("text/html"), r#"{"query": "{ users { name } }"}"#),
    );
    assert_eq!(sent.status, 406);
    assert_eq!(sent.content_type, JSON);
}

#[test]
fn a_post_body_must_be_json() {
    for content_type in [
        None,
        Some("text/plain"),
        Some("application/json; charset=latin1"),
    ] {
        let sent = send(
            App::default(),
            RequestParts {
                content_type,
                ..post(GRAPHQL, r#"{"query": "{ users { name } }"}"#)
            },
        );
        assert_eq!(sent.status, 415, "{content_type:?}");
    }
    for body in ["", "{", "{\"query\": \"\u{1}\"\u{0}"] {
        let sent = send(App::default(), post(GRAPHQL, body));
        assert_eq!(sent.status, 400, "{body:?}");
        assert_eq!(sent.content_type, JSON);
        assert!(sent.json()["errors"][0]["message"].is_string());
    }
}

#[test]
fn parameters_that_are_not_well_formed_are_422() {
    for body in [
        r#"[]"#,
        r#"{"qeury": "{ users { name } }"}"#,
        r#"{"query": {"obj": "ect"}}"#,
        r#"{"query": "{ users { name } }", "operationName": 0}"#,
        r#"{"query": "{ users { name } }", "variables": ["array"]}"#,
        r#"{"query": "{ users { name } }", "extensions": "string"}"#,
    ] {
        let sent = send(App::default(), post(GRAPHQL, body));
        assert_eq!(sent.status, 422, "{body}");
        assert_eq!(sent.content_type, JSON);
    }
    let sent = send(
        App::default(),
        get("query=%7B+users+%7B+name+%7D+%7D&variables=nope"),
    );
    assert_eq!(sent.status, 422);
}

#[test]
fn on_error_picks_the_error_behavior() {
    let failing = || App {
        fail_name: true,
        ..Default::default()
    };
    // The server default, PROPAGATE: the non-null name nulls `users`, then `data`.
    let sent = send(
        failing(),
        post(GRAPHQL, r#"{"query": "{ users { name } }"}"#),
    );
    assert_eq!(sent.status, 294);
    assert_eq!(sent.json()["data"], Value::Null);

    let sent = send(
        failing(),
        post(
            GRAPHQL,
            r#"{"query": "{ users { name } }", "onError": "NULL"}"#,
        ),
    );
    assert_eq!(sent.status, 294);
    let response = sent.json();
    assert_eq!(response["data"]["users"][0]["name"], json!("Ann"));
    assert_eq!(response["data"]["users"][1]["name"], Value::Null);

    let sent = send(
        failing(),
        get("query=%7B+users+%7B+name+%7D+%7D&onError=HALT"),
    );
    assert_eq!(sent.status, 294);
    let response = sent.json();
    assert_eq!(response["data"], Value::Null);
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);

    // A request that names none takes the endpoint's default.
    let sent = send_with(
        failing(),
        post(
            GRAPHQL,
            r#"{"query": "{ users { name } }", "onError": null}"#,
        ),
        ExecuteOptions {
            default_error_behavior: greem::ErrorBehavior::Null,
            ..Default::default()
        },
    );
    assert_eq!(sent.json()["data"]["users"][0]["name"], json!("Ann"));

    for body in [
        r#"{"query": "{ users { name } }", "onError": "null"}"#,
        r#"{"query": "{ users { name } }", "onError": "ABORT"}"#,
        r#"{"query": "{ users { name } }", "onError": true}"#,
    ] {
        let sent = send(App::default(), post(GRAPHQL, body));
        assert_eq!(sent.status, 422, "{body}");
        assert_eq!(
            sent.json()["errors"][0]["message"],
            json!("onError must be one of NULL, PROPAGATE and HALT")
        );
    }
    let sent = send(
        App::default(),
        get("query=%7B+users+%7B+name+%7D+%7D&onError=halt"),
    );
    assert_eq!(sent.status, 422);
}

#[test]
fn request_errors_are_400_for_parsing_and_422_otherwise() {
    let cases = [
        (r#"{"query": "{"}"#, 400),
        (r#"{"query": "{ nope }"}"#, 422),
        (
            r#"{"query": "query A { users { name } } query B { users { name } }"}"#,
            422,
        ),
        (
            r#"{"query": "{ users { name } }", "operationName": "Missing"}"#,
            422,
        ),
        (
            r#"{"query": "query ($id: ID!) { user(id: $id) { name } }", "variables": {"id": null}}"#,
            422,
        ),
    ];
    for accept in [GRAPHQL, LEGACY] {
        for (body, status) in cases {
            let sent = send(App::default(), post(accept, body));
            assert_eq!(sent.status, status, "{accept:?} {body}");
            assert_eq!(
                sent.content_type, GRAPHQL_RESPONSE_JSON,
                "{accept:?} {body}"
            );
            let response = sent.json();
            assert!(response.get("data").is_none(), "{response}");
            assert!(response["errors"][0]["message"].is_string(), "{response}");
        }
    }
}

#[test]
fn data_alongside_errors_is_294() {
    for (accept, content_type) in [(GRAPHQL, GRAPHQL_RESPONSE_JSON), (LEGACY, JSON)] {
        let sent = send(
            App {
                fail_email: true,
                ..Default::default()
            },
            post(accept, r#"{"query": "{ users { email } }"}"#),
        );
        assert_eq!(sent.status, 294, "{accept:?}");
        assert_eq!(sent.content_type, content_type, "{accept:?}");
        let response = sent.json();
        assert_eq!(response["data"]["users"][1], json!({"email": null}));
        assert_eq!(response["errors"][0]["message"], "email failed");
    }
    let sent = send(
        App::default(),
        post(GRAPHQL, r#"{"query": "mutation { fail }"}"#),
    );
    assert_eq!(sent.status, 294);
    assert_eq!(sent.json()["data"], Value::Null);
}

#[test]
fn incremental_delivery_needs_both_the_endpoint_and_the_client() {
    let body = r#"{"query": "{ users { name ... @defer { email } } }"}"#;
    let sent = send(
        App::default(),
        post(
            Some("multipart/mixed, application/graphql-response+json"),
            body,
        ),
    );
    assert_eq!(sent.status, 200);
    assert_eq!(sent.content_type, greem::http::MULTIPART_CONTENT_TYPE);
    assert!(sent.body.ends_with("\r\n-----\r\n"), "{}", sent.body);
    assert!(sent.body.contains("\"hasNext\":true"), "{}", sent.body);

    let sent = send(App::default(), post(GRAPHQL, body));
    assert_eq!(sent.content_type, GRAPHQL_RESPONSE_JSON);
    assert!(sent.json().get("hasNext").is_none());

    let sent = send_with(
        App::default(),
        post(Some("multipart/mixed, application/json"), body),
        ExecuteOptions::default(),
    );
    assert_eq!(sent.content_type, JSON);
}

#[test]
fn multipart_follows_the_clients_preference() {
    let sent = send(
        App::default(),
        post(
            Some("multipart/mixed;q=0.5, application/graphql-response+json"),
            r#"{"query": "{ users { name ... @defer { email } } }"}"#,
        ),
    );
    assert_eq!(sent.content_type, GRAPHQL_RESPONSE_JSON);
    assert!(sent.json().get("hasNext").is_none());

    let sent = send(
        App::default(),
        post(
            Some("multipart/mixed"),
            r#"{"query": "{ users { name } }"}"#,
        ),
    );
    assert_eq!(sent.status, 200);
    assert_eq!(sent.content_type, greem::http::MULTIPART_CONTENT_TYPE);
    assert_eq!(
        sent.body,
        "\r\n---\r\nContent-Type: application/json; charset=utf-8\r\n\r\n\
         {\"data\":{\"users\":[{\"name\":\"Ann\"},{\"name\":\"Bob\"}]}}\r\n-----\r\n"
    );

    let sent = send(
        App::default(),
        post(Some("multipart/mixed"), r#"{"query": "{ nope }"}"#),
    );
    assert_eq!(sent.status, 422);
    assert_eq!(sent.content_type, GRAPHQL_RESPONSE_JSON);
}

#[cfg(feature = "http")]
#[test]
fn the_http_feature_converts_both_ways() {
    use http_body_util::BodyExt;

    let request = http::Request::get("/graphql?query=%7B+users+%7B+name+%7D+%7D")
        .header(http::header::ACCEPT, "text/html")
        .header(http::header::ACCEPT, "application/graphql-response+json")
        .body(())
        .unwrap();
    let (parts, ()) = request.into_parts();
    let schema = build_schema();
    let response = block_on(greem::http::handle(
        &schema,
        Roots::query(QueryRoot)
            .with_mutation(MutationRoot)
            .with_subscription(SubscriptionRoot),
        App::default(),
        RequestParts::from_http(&parts, b""),
        ExecuteOptions::default(),
    ));
    let response = http::Response::from(response);
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()[http::header::CONTENT_TYPE],
        GRAPHQL_RESPONSE_JSON
    );
    let body = block_on(response.into_body().collect()).unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body,
        json!({"data": {"users": [{"name": "Ann"}, {"name": "Bob"}]}})
    );

    let request = http::Request::delete("/graphql").body(()).unwrap();
    let (parts, ()) = request.into_parts();
    let response = http::Response::from(block_on(greem::http::handle(
        &schema,
        Roots::query(QueryRoot)
            .with_mutation(MutationRoot)
            .with_subscription(SubscriptionRoot),
        App::default(),
        RequestParts::from_http(&parts, b""),
        ExecuteOptions::default(),
    )));
    assert_eq!(response.status(), 405);
    assert_eq!(response.headers()[http::header::ALLOW], "GET, POST, QUERY");
}

#[test]
fn a_subscription_needs_a_streaming_transport() {
    let sent = send(
        App::default(),
        post(GRAPHQL, r#"{"query": "subscription { ticks(count: 1) }"}"#),
    );
    assert_eq!(sent.status, 422);
    assert_eq!(sent.content_type, JSON);
    let message = sent.json()["errors"][0]["message"].clone();
    assert!(
        message.as_str().unwrap().contains("streaming transport"),
        "{message}"
    );
}
