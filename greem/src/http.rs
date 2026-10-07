//! Transport-agnostic HTTP pieces: the request body type, payload helpers
//! and a `multipart/mixed` part encoder for incremental delivery.

use crate::exec::payload::PayloadKind;
use crate::schema::{ExecutionOutput, OwnedPayload};
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

/// Whether an `Accept` header value asks for incremental delivery in the
/// format greem serializes: a `multipart/mixed` entry with no spec parameter
/// or with `incrementalSpec=v0.2`. A client that names only another format,
/// such as `deferSpec=20220824`, cannot read these payloads, so it should get
/// a single non-incremental response instead.
pub fn accepts_multipart(accept: Option<&str>) -> bool {
    accept.is_some_and(|value| {
        value.split(',').any(|entry| {
            let mut parts = entry.split(';').map(str::trim);
            if !parts
                .next()
                .is_some_and(|ty| ty.eq_ignore_ascii_case("multipart/mixed"))
            {
                return false;
            }
            parts.all(|param| {
                let (name, value) = param.split_once('=').unwrap_or((param, ""));
                let value = value.trim().trim_matches('"');
                match name.trim() {
                    name if name.eq_ignore_ascii_case("deferSpec") => false,
                    name if name.eq_ignore_ascii_case("incrementalSpec") => value == "v0.2",
                    _ => true,
                }
            })
        })
    })
}

/// Encodes one payload as a multipart part. Each part carries the delimiter
/// that follows it (the closing one once nothing follows), so a client can
/// read a streamed payload when it arrives, not when the next one starts.
pub fn multipart_part(payload: &OwnedPayload) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.json.len() + 64);
    if payload.kind != PayloadKind::Subsequent {
        out.extend_from_slice(b"\r\n---\r\n");
    }
    out.extend_from_slice(b"Content-Type: application/json; charset=utf-8\r\n\r\n");
    out.extend_from_slice(&payload.json);
    if payload.has_next == Some(true) {
        out.extend_from_slice(b"\r\n---\r\n");
    } else {
        out.extend_from_slice(b"\r\n-----\r\n");
    }
    out
}

/// Encodes a whole owned execution as one multipart body.
pub fn multipart_body(output: &ExecutionOutput) -> Vec<u8> {
    output.payloads.iter().flat_map(multipart_part).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_is_accepted_only_for_the_serialized_format() {
        let cases = [
            (
                "multipart/mixed;incrementalSpec=v0.2,application/graphql-response+json,application/json;q=0.9",
                true,
            ),
            ("application/json, text/event-stream, multipart/mixed", true),
            ("multipart/mixed", true),
            ("Multipart/Mixed; IncrementalSpec=\"v0.2\"", true),
            (
                "multipart/mixed;deferSpec=20220824, multipart/mixed;incrementalSpec=v0.2",
                true,
            ),
            (MULTIPART_CONTENT_TYPE, true),
            (
                "multipart/mixed;deferSpec=20220824,application/graphql-response+json,application/json;q=0.9",
                false,
            ),
            ("multipart/mixed;deferSpec=20220824,application/json", false),
            ("multipart/mixed;incrementalSpec=v0.3", false),
            ("application/json", false),
        ];
        for (accept, expected) in cases {
            assert_eq!(accepts_multipart(Some(accept)), expected, "{accept}");
        }
        assert!(!accepts_multipart(None));
    }
}
