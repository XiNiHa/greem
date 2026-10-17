//! The response side: status codes and media types per the GraphQL-over-HTTP
//! draft.

use super::accept::{Framing, Media, Negotiated};
use crate::error::RequestErrorKind;
use crate::exec::payload::{Payload, PayloadKind};
use futures::stream::BoxStream;
use futures::{StreamExt, future, stream};

const GRAPHQL_RESPONSE_JSON: &str = "application/graphql-response+json; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";

/// An HTTP response for an integration to send as is.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    /// The `Allow` header, set when the status is `405`.
    pub allow: Option<&'static str>,
    pub body: Body,
}

pub enum Body {
    Full(Vec<u8>),
    /// The chunks of a streamed response, each one multipart part or SSE
    /// event as the execution ships it; the execution advances only while
    /// this is polled, and dropping it cancels the execution. Headers beyond
    /// `Content-Type` are the adapter's: an event stream wants
    /// `Cache-Control: no-cache` past proxies, and an adapter that wants SSE
    /// keepalives merges comment lines (`:\n\n`) into this stream on its own
    /// timer.
    Stream(BoxStream<'static, Vec<u8>>),
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Body::Full(bytes) => f
                .debug_tuple("Full")
                .field(&String::from_utf8_lossy(bytes))
                .finish(),
            Body::Stream(_) => f.write_str("Stream(..)"),
        }
    }
}

impl Response {
    /// A request greem cannot serve as a GraphQL-over-HTTP response, such
    /// as one it cannot read or one with no acceptable media type; the body
    /// is a plain JSON `errors` list, never `application/graphql-response+json`.
    pub(super) fn rejected(
        status: u16,
        allow: Option<&'static str>,
        message: impl Into<String>,
    ) -> Self {
        let body = serde_json::json!({ "errors": [{ "message": message.into() }] });
        Response {
            status,
            content_type: JSON,
            allow,
            body: Body::Full(serde_json::to_vec(&body).expect("serialize errors")),
        }
    }

    /// A whole GraphQL response. Under legacy `application/json`, only a
    /// `2xx` keeps that media type.
    pub(super) fn single(media: Media, encoded: Encoded) -> Self {
        let content_type = match media {
            Media::Json if (200..300).contains(&encoded.status) => JSON,
            _ => GRAPHQL_RESPONSE_JSON,
        };
        Response {
            status: encoded.status,
            content_type,
            allow: None,
            body: Body::Full(encoded.bytes),
        }
    }

    /// A streamed response: always 200, since its status goes out before the
    /// later payloads exist. An event stream ends with the `complete` event,
    /// which is all an event source without events sends.
    pub(super) fn streamed(framing: Framing, chunks: BoxStream<'static, Vec<u8>>) -> Self {
        let (content_type, chunks) = match framing {
            Framing::Multipart => (super::MULTIPART_CONTENT_TYPE, chunks),
            Framing::EventStream => (
                super::SSE_CONTENT_TYPE,
                chunks
                    .chain(stream::once(future::ready(super::sse_complete())))
                    .boxed(),
            ),
        };
        Response {
            status: 200,
            content_type,
            allow: None,
            body: Body::Stream(chunks),
        }
    }

    /// The response for an execution's first encoded payload and the
    /// payloads after it, in the framing the encoder chose.
    pub(super) fn from_encoded(
        media: Media,
        first: Encoded,
        rest: BoxStream<'static, Vec<u8>>,
    ) -> Self {
        match first.framing {
            Some(framing) => Response::streamed(
                framing,
                stream::once(future::ready(first.bytes)).chain(rest).boxed(),
            ),
            None => Response::single(media, first),
        }
    }
}

/// A payload encoded while the execution still borrows it: a chunk of a
/// streamed response, or the whole JSON body.
pub(super) struct Encoded {
    /// The framing the payload was encoded for; `None` is one JSON body.
    pub framing: Option<Framing>,
    pub status: u16,
    pub bytes: Vec<u8>,
}

/// Encodes one payload as negotiated: for a client reading only
/// `text/event-stream` every payload is an event, for one reading only
/// `multipart/mixed` every `2xx` payload is a part, and otherwise an
/// incremental payload takes the negotiated incremental framing and a whole
/// response is one JSON body.
pub(super) fn encode(payload: Payload<'_>, negotiated: Negotiated) -> Encoded {
    let status = status(&payload);
    let framing = match negotiated.media {
        Media::Streaming(Framing::EventStream) => Some(Framing::EventStream),
        Media::Streaming(Framing::Multipart) if (200..300).contains(&status) => {
            Some(Framing::Multipart)
        }
        _ if payload.has_next().is_some() => Some(
            negotiated
                .incremental
                .expect("an incremental payload needs a negotiated incremental framing"),
        ),
        _ => None,
    };
    Encoded {
        framing,
        status,
        bytes: match framing {
            Some(Framing::Multipart) => super::multipart_part(&payload),
            Some(Framing::EventStream) => super::sse_event(&payload),
            None => serde_json::to_vec(&payload).expect("serialize payload"),
        },
    }
}

fn status(payload: &Payload<'_>) -> u16 {
    match payload.kind() {
        PayloadKind::RequestError(RequestErrorKind::Parse) => 400,
        PayloadKind::RequestError(_) => 422,
        _ if payload.errors().is_empty() => 200,
        _ => 294,
    }
}
