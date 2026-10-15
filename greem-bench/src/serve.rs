//! The greem implementation served over HTTP for the driver: `POST /graphql`
//! over the bench `App`, the workload header selecting which workload's fetch
//! latency a request pays.

use crate::app::{App, QueryRoot, Schema, build_schema};
use crate::workload::{self, Workload};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use greem::{ExecuteOptions, IncrementalDelivery, Roots};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

struct Server {
    schema: Schema,
    apps: HashMap<String, App>,
}

/// The router over the workloads under `dir`.
pub fn router(dir: &Path) -> Router {
    let apps = Workload::all_in(dir)
        .into_iter()
        .map(|workload| (workload.name.clone(), App::new(workload.latency)))
        .collect();
    let server = Arc::new(Server {
        schema: build_schema(),
        apps,
    });
    Router::new()
        .route("/graphql", any(graphql))
        .with_state(server)
}

/// Serves the workloads under `WORKLOADS` on `PORT` (default 4000) until
/// killed.
pub async fn serve() {
    let workloads = std::env::var("WORKLOADS").unwrap_or_else(|_| workload::DIR.to_owned());
    let port = std::env::var("PORT")
        .ok()
        .map(|p| p.parse::<u16>().expect("PORT is a port number"))
        .unwrap_or(4000);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    println!("greem bench server listening on http://127.0.0.1:{port}/graphql");
    axum::serve(listener, router(Path::new(&workloads)))
        .await
        .expect("serve");
}

async fn graphql(State(server): State<Arc<Server>>, parts: Parts, body: Bytes) -> Response {
    let app = parts
        .headers
        .get(workload::HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|name| server.apps.get(name))
        .cloned()
        .unwrap_or_default();
    let options = ExecuteOptions {
        incremental: IncrementalDelivery::Enabled,
        ..Default::default()
    };
    let request = greem::http::RequestParts::from_http(&parts, &body);
    let response = greem::http::handle(
        &server.schema,
        Roots::query(QueryRoot),
        app,
        request,
        options,
    )
    .await;
    axum::http::Response::from(response).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::merge::merge;
    use crate::workload::{self, Workload};

    async fn post(workload: &Workload, accept: &str) -> (StatusCode, String, String) {
        let body = greem::http::Request::new(workload.document.clone())
            .with_variables(workload.variables_at(workload.smallest()));
        let request = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, accept)
            .header(workload::HEADER, &workload.name)
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let response = router(Path::new(workload::DIR))
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            content_type,
            String::from_utf8(bytes.to_vec()).unwrap(),
        )
    }

    #[tokio::test]
    async fn a_plain_workload_answers_its_oracle_as_one_json_body() {
        let workload = Workload::load("wide");
        let (status, content_type, body) = post(&workload, "application/json").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "application/json; charset=utf-8");
        let response: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(response, workload.oracle());
    }

    #[tokio::test]
    async fn an_incremental_workload_streams_multipart_that_merges_to_its_oracle() {
        let workload = Workload::load("deferred");
        let (status, content_type, body) = post(
            &workload,
            "multipart/mixed;incrementalSpec=v0.2, application/json",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, greem::http::MULTIPART_CONTENT_TYPE);
        let payloads: Vec<serde_json::Value> = body
            .split("\r\n---\r\n")
            .filter(|part| !part.is_empty() && !part.starts_with("--"))
            .map(|part| {
                let json = part
                    .split("\r\n\r\n")
                    .nth(1)
                    .unwrap()
                    .trim_end_matches("\r\n-----\r\n");
                serde_json::from_str(json).unwrap()
            })
            .collect();
        assert!(payloads.len() > 1);
        assert_eq!(merge(payloads), workload.oracle());
    }

    #[tokio::test]
    async fn the_workload_header_selects_the_fetch_latency() {
        let workload = Workload::load("fetch");
        let started = std::time::Instant::now();
        let (status, _, _) = post(&workload, "application/json").await;
        assert_eq!(status, StatusCode::OK);
        let two_fetches = workload.latency["Query.users"] + workload.latency["User.posts"];
        assert!(started.elapsed() >= two_fetches);
    }
}
