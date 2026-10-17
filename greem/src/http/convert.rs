//! The `http` feature: [`RequestParts`] from `http` request parts, and
//! [`Response`] into an `http` response whose [`Body`] is an
//! `http_body::Body`, for axum, hyper and the other servers built on them.

use super::{Body, RequestParts, Response};
use bytes::Bytes;
use http_body::{Frame, SizeHint};
use std::borrow::Cow;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

impl<'a> RequestParts<'a> {
    /// The parts of an `http` request whose body has been read in full.
    /// Header values that are not visible ASCII count as absent, and repeated
    /// `Accept` field lines are joined into one list.
    pub fn from_http(parts: &'a ::http::request::Parts, body: &'a [u8]) -> Self {
        let header = |name| {
            parts
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
        };
        let accept: Vec<&str> = parts
            .headers
            .get_all(::http::header::ACCEPT)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        RequestParts {
            method: parts.method.as_str(),
            query_string: parts.uri.query(),
            content_type: header(::http::header::CONTENT_TYPE),
            accept: match accept.as_slice() {
                [] => None,
                [accept] => Some(Cow::Borrowed(*accept)),
                _ => Some(Cow::Owned(accept.join(", "))),
            },
            body,
        }
    }
}

impl From<Response> for ::http::Response<Body> {
    fn from(response: Response) -> Self {
        let mut builder = ::http::Response::builder()
            .status(response.status)
            .header(::http::header::CONTENT_TYPE, response.content_type);
        if let Some(allow) = response.allow {
            builder = builder.header(::http::header::ALLOW, allow);
        }
        builder
            .body(response.body)
            .expect("greem's status codes and header values are valid")
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.get_mut() {
            Body::Full(bytes) if bytes.is_empty() => Poll::Ready(None),
            Body::Full(bytes) => {
                Poll::Ready(Some(Ok(Frame::data(Bytes::from(std::mem::take(bytes))))))
            }
            Body::Stream(chunks) => chunks
                .as_mut()
                .poll_next(cx)
                .map(|chunk| chunk.map(|chunk| Ok(Frame::data(Bytes::from(chunk))))),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, Body::Full(bytes) if bytes.is_empty())
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            Body::Full(bytes) => SizeHint::with_exact(bytes.len() as u64),
            Body::Stream(_) => SizeHint::default(),
        }
    }
}
