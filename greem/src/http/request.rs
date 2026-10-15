//! The request side: which methods and bodies greem reads, and the
//! well-formedness of the parameters they carry.

use super::Request;
use super::accept::{Specificity, application, entry};
use super::response::Response;
use serde_json::{Map, Value};
use std::borrow::Cow;

/// The parts of an HTTP request that [`handle`](super::handle) reads: header
/// values as sent, and the body already read in full.
#[derive(Clone, Debug, Default)]
pub struct RequestParts<'a> {
    pub method: &'a str,
    /// The URI's query component, without the leading `?`.
    pub query_string: Option<&'a str>,
    pub content_type: Option<&'a str>,
    /// Every `Accept` field line, joined with `, ` when there are several.
    pub accept: Option<Cow<'a, str>>,
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Method {
    Get,
    Post,
    Query,
}

impl RequestParts<'_> {
    pub(super) fn method(&self) -> Result<Method, Response> {
        match self.method {
            "GET" => Ok(Method::Get),
            "POST" => Ok(Method::Post),
            "QUERY" => Ok(Method::Query),
            other => Err(Response::rejected(
                405,
                Some("GET, POST, QUERY"),
                format!("method {other} is not allowed"),
            )),
        }
    }

    /// The GraphQL-over-HTTP request: from the query string of a GET, from
    /// the JSON body of a POST or QUERY.
    pub(super) fn request(&self, method: Method) -> Result<Request, Response> {
        let params = match method {
            Method::Get => query_params(self.query_string.unwrap_or_default())?,
            Method::Post | Method::Query => self.body_params()?,
        };
        well_formed(params).map_err(|message| Response::rejected(422, None, message))
    }

    fn body_params(&self) -> Result<Map<String, Value>, Response> {
        match self.content_type {
            Some(content_type) if is_json(content_type) => {}
            Some(content_type) => {
                return Err(Response::rejected(
                    415,
                    None,
                    format!("unsupported content type {content_type}; send application/json"),
                ));
            }
            None => {
                return Err(Response::rejected(
                    415,
                    None,
                    format!(
                        "a {} request needs the content type application/json",
                        self.method
                    ),
                ));
            }
        }
        match serde_json::from_slice(self.body) {
            Ok(Value::Object(params)) => Ok(params),
            Ok(_) => Err(Response::rejected(
                422,
                None,
                "the request body must be a JSON object",
            )),
            Err(e) => Err(Response::rejected(
                400,
                None,
                format!("the request body is not JSON: {e}"),
            )),
        }
    }
}

/// `application/json`, in UTF-8 if a charset is named.
fn is_json(content_type: &str) -> bool {
    entry(content_type).is_some_and(|entry| application(&entry, "json") == Some(Specificity::Exact))
}

/// The parameters of a GET's query string, `variables` and `extensions`
/// decoded from their JSON text. An empty value is the same as an absent
/// one, and the first of repeated names wins.
fn query_params(query_string: &str) -> Result<Map<String, Value>, Response> {
    let mut params = Map::new();
    for pair in query_string.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let (name, value) = (form_decode(name), form_decode(value));
        if value.is_empty() || params.contains_key(&name) {
            continue;
        }
        let value = match name.as_str() {
            "variables" | "extensions" => serde_json::from_str(&value).map_err(|e| {
                Response::rejected(422, None, format!("{name} is not a JSON-encoded map: {e}"))
            })?,
            _ => Value::String(value),
        };
        params.insert(name, value);
    }
    Ok(params)
}

/// `application/x-www-form-urlencoded` decoding of one name or value.
fn form_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A well-formed GraphQL-over-HTTP request: a string `query`, and an
/// `operationName` string and `variables` and `extensions` maps where given.
fn well_formed(mut params: Map<String, Value>) -> Result<Request, String> {
    let query = match params.remove("query") {
        Some(Value::String(query)) => query,
        Some(_) => return Err("query must be a string".into()),
        None => return Err("the request has no query".into()),
    };
    let operation_name = match params.remove("operationName") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name),
        Some(_) => return Err("operationName must be a string".into()),
    };
    Ok(Request {
        query: Some(query),
        operation_name,
        variables: map_param(params.remove("variables"), "variables")?,
        extensions: map_param(params.remove("extensions"), "extensions")?,
    })
}

fn map_param(value: Option<Value>, name: &str) -> Result<Option<Value>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value @ Value::Object(_)) => Ok(Some(value)),
        Some(_) => Err(format!("{name} must be a map")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_decoding() {
        assert_eq!(form_decode("a+b%20c%7B%7D"), "a b c{}");
        assert_eq!(form_decode("%E2%9C%93"), "✓");
        assert_eq!(form_decode("100%"), "100%");
        assert_eq!(form_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn get_parameters_come_from_the_query_string() {
        let params = query_params(
            "query=query+Q(%24id%3A+ID!)+%7B+__typename+%7D&operationName=Q&variables=%7B%22id%22%3A1%7D&extensions=&query=ignored",
        )
        .unwrap();
        let request = well_formed(params).unwrap();
        assert_eq!(
            request.query.as_deref(),
            Some("query Q($id: ID!) { __typename }")
        );
        assert_eq!(request.operation_name.as_deref(), Some("Q"));
        assert_eq!(request.variables, Some(serde_json::json!({"id": 1})));
        assert_eq!(request.extensions, None);
    }

    #[test]
    fn parameters_of_the_wrong_type_are_not_well_formed() {
        for body in [
            r#"{}"#,
            r#"{"query": 1}"#,
            r#"{"query": "{ a }", "operationName": 1}"#,
            r#"{"query": "{ a }", "variables": "{}"}"#,
            r#"{"query": "{ a }", "extensions": []}"#,
        ] {
            let params = serde_json::from_str(body).unwrap();
            assert!(well_formed(params).is_err(), "{body}");
        }
        let params =
            serde_json::from_str(r#"{"query": "{ a }", "operationName": null, "variables": null}"#)
                .unwrap();
        assert!(well_formed(params).is_ok());
    }

    #[test]
    fn json_content_types() {
        assert!(is_json("application/json"));
        assert!(is_json("Application/JSON; charset=\"UTF-8\""));
        assert!(is_json("application/json; boundary=x"));
        assert!(!is_json("application/json; charset=utf-16"));
        assert!(!is_json("application/graphql"));
        assert!(!is_json("text/plain"));
        assert!(!is_json("*/*"));
        assert!(!is_json("application/*"));
    }
}
