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
use crate::exec::state::{ErrorBehavior, ExecuteOptions, IncrementalDelivery};
use crate::resolver::Outputs;
use crate::schema::{Roots, Schema, SchemaInfo};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A GraphQL-over-HTTP request body: the four standard parameters and the
/// `onError` attribute of graphql-spec #1163 (unmerged, October 2026), the
/// error behavior the client asks for. A request that names none gets
/// [`ExecuteOptions::default_error_behavior`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub query: String,
    #[serde(default)]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<Map<String, Value>>,
    #[serde(default)]
    pub extensions: Option<Map<String, Value>>,
    #[serde(default)]
    pub on_error: Option<ErrorBehavior>,
}

impl Request {
    pub fn new(query: impl Into<String>) -> Self {
        Request {
            query: query.into(),
            operation_name: None,
            variables: None,
            extensions: None,
            on_error: None,
        }
    }

    pub fn with_on_error(mut self, on_error: ErrorBehavior) -> Self {
        self.on_error = Some(on_error);
        self
    }

    pub fn with_variables(mut self, variables: Map<String, Value>) -> Self {
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
    if payload.kind() != PayloadKind::Incremental {
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

/// The `Content-Type` header value for a server-sent events response, the
/// framing of graphql-sse's distinct connections mode
/// (<https://github.com/enisdenjo/graphql-sse/blob/master/PROTOCOL.md>).
pub const SSE_CONTENT_TYPE: &str = "text/event-stream; charset=utf-8";

/// Encodes one payload as graphql-sse's `next` event, serializing it
/// straight into the `data` line.
pub fn sse_event(payload: &Payload<'_>) -> Vec<u8> {
    let mut out = Vec::from(b"event: next\ndata: ");
    serde_json::to_writer(&mut out, payload).expect("serialize payload");
    out.extend_from_slice(b"\n\n");
    out
}

/// graphql-sse's `complete` event, which ends every event stream. It carries
/// an empty `data` line because a browser `EventSource` does not fire an
/// event without one.
pub fn sse_complete() -> Vec<u8> {
    b"event: complete\ndata:\n\n".to_vec()
}

/// Serves one GraphQL-over-HTTP request: GET, POST or QUERY, negotiated from
/// its `Accept` header, answered as one JSON response or, when the operation
/// defers or streams and the client prefers `multipart/mixed` or
/// `text/event-stream` at least as much, as a stream of parts or events.
/// A client that reads only `multipart/mixed` gets any successful result as
/// parts; one that reads only `text/event-stream` gets every response as
/// events, request errors included (ADR-0014). `options.incremental` is
/// whether this endpoint allows incremental delivery at all; a request also
/// has to accept it.
///
/// Adapters serving `text/event-stream` should add `Cache-Control: no-cache`,
/// and may merge keepalive comment lines into [`Body::Stream`] on a timer.
pub async fn handle<I, C, Q, M, S>(
    schema: &Schema<I, C, Q, M, S>,
    roots: Roots<Q, M, S>,
    ctx: C,
    request: RequestParts<'_>,
    options: ExecuteOptions,
) -> Response
where
    I: SchemaInfo,
    C: Send + Sync + 'static,
    Q: Outputs<I::Query, C> + Send + Sync + 'static,
    M: Outputs<I::Mutation, C> + Send + Sync + 'static,
    S: Outputs<I::Subscription, C> + Send + Sync + 'static,
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
    let operation = match request
        .request(method)
        .map(|request| schema.operation(request))
    {
        Ok(Ok(operation)) => operation,
        Ok(Err(errors)) => {
            let encoded = response::encode(errors.payload(), negotiated);
            return Response::from_encoded(negotiated.media, encoded, stream::empty().boxed());
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
    // A subscription's response stream can only ride SSE here, so once the
    // client reads it at all, everything after this point is events.
    let negotiated = if operation.is_subscription() {
        if !negotiated.event_stream {
            return Response::rejected(
                406,
                None,
                "a subscription needs an Accept header that includes text/event-stream",
            );
        }
        accept::Negotiated {
            media: accept::Media::Streaming(accept::Framing::EventStream),
            ..negotiated
        }
    } else {
        negotiated
    };
    let options = ExecuteOptions {
        incremental: if negotiated.incremental.is_some() {
            options.incremental
        } else {
            IncrementalDelivery::Disabled
        },
        ..options
    };
    let payloads = schema.execute_stream(roots, ctx, operation, options, move |payload| {
        response::encode(payload, negotiated)
    });
    // Every payload of an event stream is an event, so its response needs no
    // first payload: the headers go out before the event source is polled,
    // and an event source without events is just `complete`. Any other
    // framing is decided by the first payload.
    if negotiated.media == accept::Media::Streaming(accept::Framing::EventStream) {
        return Response::streamed(
            accept::Framing::EventStream,
            payloads.map(|payload| payload.bytes).boxed(),
        );
    }
    let mut payloads = Box::pin(payloads);
    let first = payloads
        .next()
        .await
        .expect("an execution ships at least one payload");
    Response::from_encoded(
        negotiated.media,
        first,
        payloads.map(|payload| payload.bytes).boxed(),
    )
}
