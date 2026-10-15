//! GraphQL over HTTP without an HTTP crate: [`handle`] serves one request
//! from its [`RequestParts`] and answers a [`Response`] with the status codes
//! and media types of the GraphQL-over-HTTP draft
//! (<https://graphql.github.io/graphql-over-http/draft/>), even where
//! graphql-http's audits expect otherwise; see
//! docs/adr/0012-http-follows-the-graphql-over-http-draft.md. QUERY
//! (RFC 10008) is served like POST but for queries only, as
//! graphql/graphql-over-http#411 proposes. The `http` feature converts both
//! to and from the `http` crate's types.

mod accept;
#[cfg(feature = "http")]
mod convert;
mod request;
mod response;

pub use request::RequestParts;
pub use response::{Body, Response};

use crate::exec::payload::{Payload, PayloadKind};
use crate::exec::state::{ExecuteOptions, IncrementalDelivery};
use crate::resolver::Outputs;
use crate::schema::{Roots, Schema, SchemaInfo};
use futures::{StreamExt, future, stream};
use serde::{Deserialize, Serialize};

/// A GraphQL-over-HTTP request body.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default, rename = "operationName")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
    #[serde(default)]
    pub extensions: Option<serde_json::Value>,
}

impl Request {
    pub fn new(query: impl Into<String>) -> Self {
        Request {
            query: Some(query.into()),
            ..Default::default()
        }
    }

    pub fn with_variables(mut self, variables: serde_json::Value) -> Self {
        self.variables = Some(variables);
        self
    }

    pub fn with_operation_name(mut self, name: impl Into<String>) -> Self {
        self.operation_name = Some(name.into());
        self
    }
}

/// The boundary used for `multipart/mixed` incremental responses.
pub const MULTIPART_BOUNDARY: &str = "-";

/// The `Content-Type` header value for an incremental response. Payloads use
/// the `pending`/`completed` format that Apollo's incremental delivery v0.2
/// (<https://specs.apollo.dev/incremental/v0.2>) names `incrementalSpec=v0.2`.
pub const MULTIPART_CONTENT_TYPE: &str = "multipart/mixed; boundary=\"-\"; incrementalSpec=v0.2";

/// Encodes one payload as a multipart part, serializing it straight into the
/// part. Each part carries the delimiter that follows it (the closing one
/// once nothing follows), so a client can read a streamed payload when it
/// arrives, not when the next one starts.
pub fn multipart_part(payload: &Payload<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    if payload.kind() != PayloadKind::Subsequent {
        out.extend_from_slice(b"\r\n---\r\n");
    }
    out.extend_from_slice(b"Content-Type: application/json; charset=utf-8\r\n\r\n");
    serde_json::to_writer(&mut out, payload).expect("serialize payload");
    if payload.has_next() == Some(true) {
        out.extend_from_slice(b"\r\n---\r\n");
    } else {
        out.extend_from_slice(b"\r\n-----\r\n");
    }
    out
}

/// Serves one GraphQL-over-HTTP request: GET, POST or QUERY, negotiated from
/// its `Accept` header, answered as one JSON response or, when the operation
/// defers or streams and the client prefers `multipart/mixed` at least as
/// much, as a stream of parts; a client that reads only `multipart/mixed`
/// gets any successful result as parts. `options.incremental` is whether this
/// endpoint allows incremental delivery at all; a request also has to accept
/// it.
pub async fn handle<I, C, Q, M>(
    schema: &Schema<I, C, Q, M>,
    roots: Roots<Q, M>,
    ctx: C,
    request: RequestParts<'_>,
    options: ExecuteOptions,
) -> Response
where
    I: SchemaInfo,
    C: Send + Sync + 'static,
    Q: Outputs<I::Query, C> + Send + Sync + 'static,
    M: Outputs<I::Mutation, C> + Send + Sync + 'static,
{
    let method = match request.method() {
        Ok(method) => method,
        Err(rejected) => return rejected,
    };
    let Some(negotiated) = accept::negotiate(request.accept.as_deref()) else {
        return Response::rejected(
            406,
            None,
            "send an Accept header that includes application/graphql-response+json",
        );
    };
    let media = negotiated.media;
    let operation = match request
        .request(method)
        .map(|request| schema.operation(request))
    {
        Ok(Ok(operation)) => operation,
        Ok(Err(errors)) => {
            return Response::single(media, response::encode(errors.payload(), media));
        }
        Err(rejected) => return rejected,
    };
    match method {
        request::Method::Get if operation.is_mutation() => {
            return Response::rejected(405, Some("POST"), "mutations need a POST request");
        }
        request::Method::Query if operation.is_mutation() => {
            return Response::rejected(422, None, "mutations need a POST request");
        }
        _ => {}
    }
    let options = ExecuteOptions {
        incremental: if negotiated.multipart {
            options.incremental
        } else {
            IncrementalDelivery::Disabled
        },
        ..options
    };
    let mut payloads =
        Box::pin(
            schema.execute_stream(roots, ctx, operation, options, move |payload| {
                response::encode(payload, media)
            }),
        );
    let first = payloads
        .next()
        .await
        .expect("an execution ships at least one payload");
    if first.multipart {
        Response::multipart(
            stream::once(future::ready(first.bytes))
                .chain(payloads.map(|payload| payload.bytes))
                .boxed(),
        )
    } else {
        Response::single(media, first)
    }
}
