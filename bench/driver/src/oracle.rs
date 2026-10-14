//! Whether an implementation reproduces a workload's oracle at its smallest
//! size: one JSON response, or multipart payloads that merge to it.

use crate::http::{Request, Response, connection, send};
use greem_bench::merge::merge;
use greem_bench::workload::Workload;

pub async fn check(url: &str, workload: &Workload) -> Result<(), String> {
    let request = Request::new(url, workload, workload.smallest());
    let response = send(&connection(), &request).await?;
    verify(&response, workload)
}

fn verify(response: &Response, workload: &Workload) -> Result<(), String> {
    if response.status != 200 {
        return Err(format!(
            "status {}: {}",
            response.status,
            String::from_utf8_lossy(&response.body)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    let payloads = response.payloads()?;
    if workload.incremental() {
        if !response.content_type.starts_with("multipart/mixed") {
            return Err(format!("not multipart: {}", response.content_type));
        }
        if payloads.len() < 2 {
            return Err("delivered in one payload".into());
        }
    } else if payloads.len() != 1 {
        return Err(format!("{} payloads to a plain request", payloads.len()));
    }
    if merge(payloads) == workload.oracle() {
        Ok(())
    } else {
        Err("oracle mismatch".into())
    }
}
