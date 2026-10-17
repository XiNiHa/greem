//! Running one workload at one size: what the bench and the expected-output
//! test share.

use crate::app::{App, Plan, QueryRoot, Schema};
use crate::workload::Workload;
use futures::{Stream, StreamExt};
use greem::{Document, ExecuteOptions, IncrementalDelivery, Operation, Payload, Roots};
use std::sync::Arc;

/// The runtime every consumer drives a run on: one thread, with the timer
/// fetch latency needs.
pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("tokio runtime")
}

pub fn options(workload: &Workload) -> ExecuteOptions {
    ExecuteOptions {
        incremental: if workload.incremental() {
            IncrementalDelivery::Enabled
        } else {
            IncrementalDelivery::Disabled
        },
        ..Default::default()
    }
}

pub fn app(workload: &Workload) -> App {
    App::new(workload.latency.clone())
}

pub fn parse(schema: &Schema, workload: &Workload) -> Arc<Document> {
    schema
        .parse(&workload.document)
        .unwrap_or_else(|e| panic!("{}: {}", workload.name, e.into_payload().as_str()))
}

pub fn operation(document: &Arc<Document>, workload: &Workload, size: u32) -> Operation {
    Operation {
        document: document.clone(),
        operation_name: None,
        variables: workload.variables_at(size),
    }
}

/// Plans one run: variable coercion, tree build and lookbehind planning, the
/// per-request work before any resolver runs.
pub fn plan(schema: &Schema, workload: &Workload, app: &App, op: &Operation) -> Plan {
    schema
        .plan(op, app, options(workload))
        .unwrap_or_else(|e| panic!("{}: {}", workload.name, e.into_payload().as_str()))
}

/// Executes one run, handing every payload to `encode` while it is borrowed.
pub fn execute<T: Send, E: for<'p> FnMut(Payload<'p>) -> T + Send>(
    schema: &Schema,
    workload: &Workload,
    op: Operation,
    encode: E,
) -> impl Stream<Item = T> + Send {
    schema.execute_stream(
        Roots::query(QueryRoot),
        app(workload),
        op,
        options(workload),
        encode,
    )
}

/// The text path: parses, plans and executes one run from the request body.
pub fn execute_request<T: Send, E: for<'p> FnMut(Payload<'p>) -> T + Send>(
    schema: &Schema,
    workload: &Workload,
    size: u32,
    encode: E,
) -> impl Stream<Item = T> + Send {
    let request = greem::http::Request::new(workload.document.clone())
        .with_variables(workload.variables_at(size));
    schema.execute_request_stream(
        Roots::query(QueryRoot),
        app(workload),
        request,
        options(workload),
        encode,
    )
}

/// An encoder writing each payload as JSON into one reused buffer; yields the
/// payload's length.
pub fn encode_json() -> impl for<'p> FnMut(Payload<'p>) -> usize + Send {
    let mut buf = Vec::new();
    move |payload| {
        buf.clear();
        serde_json::to_writer(&mut buf, &payload).expect("payload serializes");
        buf.len()
    }
}

/// Every payload of one run as JSON, in order.
pub async fn payloads(schema: &Schema, workload: &Workload, size: u32) -> Vec<serde_json::Value> {
    let document = parse(schema, workload);
    execute(
        schema,
        workload,
        operation(&document, workload, size),
        |payload| serde_json::to_value(payload).expect("payload serializes"),
    )
    .collect()
    .await
}
