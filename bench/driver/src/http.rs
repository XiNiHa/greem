//! One request to one implementation: the body a workload and size produce,
//! the `Accept` and `x-workload` headers, and the response read as it streams
//! so the first complete payload of an incremental response is timed.

use crate::multipart;
use futures::StreamExt;
use greem_bench::workload::{self, Workload};
use std::time::{Duration, Instant};

const JSON: &str = "application/json";
const MULTIPART: &str = "multipart/mixed;incrementalSpec=v0.2, application/json";

/// Everything one cell's requests share.
pub struct Request {
    pub url: String,
    pub workload: String,
    pub incremental: bool,
    pub body: Vec<u8>,
}

impl Request {
    pub fn new(url: &str, workload: &Workload, size: u32) -> Request {
        let body = serde_json::json!({
            "query": workload.document,
            "variables": workload.variables_at(size),
        });
        Request {
            url: url.to_owned(),
            workload: workload.name.clone(),
            incremental: workload.incremental(),
            body: serde_json::to_vec(&body).expect("request serializes"),
        }
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub elapsed: Duration,
    /// When the first complete payload had arrived, for incremental targets.
    pub first_payload: Option<Duration>,
}

impl Response {
    /// How many multipart payloads arrived, for an incremental response.
    pub fn payload_count(&self) -> Option<usize> {
        multipart::boundary(&self.content_type)
            .map(|boundary| multipart::complete_parts(&boundary, &self.body).len())
    }

    /// The payloads of an incremental response, or the one JSON body.
    pub fn payloads(&self) -> Result<Vec<serde_json::Value>, String> {
        let bodies: Vec<&[u8]> = match multipart::boundary(&self.content_type) {
            Some(boundary) => multipart::complete_parts(&boundary, &self.body),
            None => vec![&self.body],
        };
        bodies
            .into_iter()
            .map(|body| {
                serde_json::from_slice(body).map_err(|e| format!("payload is not JSON: {e}"))
            })
            .collect()
    }
}

/// One HTTP/1.1 connection, kept alive across requests.
pub fn connection() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(1)
        .build()
        .expect("http client")
}

pub async fn send(connection: &reqwest::Client, request: &Request) -> Result<Response, String> {
    let started = Instant::now();
    let response = connection
        .post(&request.url)
        .header("content-type", JSON)
        .header("accept", if request.incremental { MULTIPART } else { JSON })
        .header(workload::HEADER, &request.workload)
        .body(request.body.clone())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let boundary = request
        .incremental
        .then(|| multipart::boundary(&content_type))
        .flatten();
    let mut body = Vec::new();
    let mut first_payload = None;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        body.extend_from_slice(&chunk);
        if first_payload.is_none()
            && let Some(boundary) = &boundary
            && !multipart::complete_parts(boundary, &body).is_empty()
        {
            first_payload = Some(started.elapsed());
        }
    }
    Ok(Response {
        status,
        content_type,
        body,
        elapsed: started.elapsed(),
        first_payload,
    })
}
