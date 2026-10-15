//! The response side: status codes and media types per the GraphQL-over-HTTP
//! draft.

use super::accept::Media;
use crate::error::RequestErrorKind;
use crate::exec::payload::{Payload, PayloadKind};
use futures::stream::BoxStream;

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
    /// The parts of a `multipart/mixed` response, each one item as the
    /// execution ships it; the execution advances only while this is polled.
    Parts(BoxStream<'static, Vec<u8>>),
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Body::Full(bytes) => f
                .debug_tuple("Full")
                .field(&String::from_utf8_lossy(bytes))
                .finish(),
            Body::Parts(_) => f.write_str("Parts(..)"),
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

    pub(super) fn multipart(parts: BoxStream<'static, Vec<u8>>) -> Self {
        Response {
            status: 200,
            content_type: super::MULTIPART_CONTENT_TYPE,
            allow: None,
            body: Body::Parts(parts),
        }
    }
}

/// A payload encoded while the execution still borrows it: a multipart part
/// when the response is multipart, otherwise the whole JSON body.
pub(super) struct Encoded {
    pub multipart: bool,
    pub status: u16,
    pub bytes: Vec<u8>,
}

pub(super) fn encode(payload: Payload<'_>, media: Media) -> Encoded {
    let status = status(&payload);
    let multipart =
        payload.has_next().is_some() || (media == Media::Multipart && (200..300).contains(&status));
    Encoded {
        multipart,
        status,
        bytes: if multipart {
            super::multipart_part(&payload)
        } else {
            serde_json::to_vec(&payload).expect("serialize payload")
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
