//! The depth-first reference executor: one field at a time, one object at a
//! time, building `serde_json::Value` directly and bubbling nulls the spec's
//! way. It shares only the execution tree and the planning pass with the
//! breadth-first executor and exists to prove that executor equivalent.

use greem::__private::SchemaInfo;
use greem::__private::{RefCompletion, RefShared};
use greem::{ErrorBehavior, ExecuteOptions, Operation, Outputs, Roots, Schema, Shape};
use std::sync::{Arc, Mutex};

/// A non-incremental response plus the number of resolver invocations.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceOutput {
    pub response: serde_json::Value,
    pub calls: u64,
}

/// Executes `op` depth-first over the same Plan table the breadth-first
/// executor would build. `@defer` and `@stream` are ignored.
pub async fn execute<I, C, Q, M>(
    schema: &Schema<I, C, Q, M>,
    roots: Roots<Q, M>,
    ctx: C,
    op: Operation,
    options: ExecuteOptions,
) -> ReferenceOutput
where
    I: SchemaInfo,
    C: Send + Sync + 'static,
    Q: Outputs<I::Query, C> + Send + Sync,
    M: Outputs<I::Mutation, C> + Send + Sync,
{
    let app = Arc::new(ctx);
    let (table, introspection, is_mutation) =
        match schema.__prepare_reference(&op, app.clone(), options) {
            Ok(prepared) => prepared,
            Err(errors) => {
                return ReferenceOutput {
                    response: serde_json::json!({ "errors": errors.0 }),
                    calls: 0,
                };
            }
        };
    let shared = RefShared {
        table,
        app,
        behavior: options.error_behavior,
        introspection,
        errors: Mutex::new(Vec::new()),
        calls: Mutex::new(0),
        serial_root: is_mutation,
    };
    let rc = RefCompletion::new(&shared, 0, Vec::new(), Shape::new(&[], false), Vec::new());
    let Roots { query, mutation } = roots;
    let data = if is_mutation {
        <M as Outputs<I::Mutation, C>>::__reference(mutation, &rc).await
    } else {
        <Q as Outputs<I::Query, C>>::__reference(query, &rc).await
    };
    let mut errors = std::mem::take(&mut *shared.errors.lock().unwrap());
    let calls = *shared.calls.lock().unwrap();
    let mut data = data.unwrap_or(serde_json::Value::Null);
    if options.error_behavior == ErrorBehavior::Halt && !errors.is_empty() {
        data = serde_json::Value::Null;
        errors.truncate(1);
    }
    let mut response = serde_json::Map::new();
    response.insert("data".into(), data);
    if !errors.is_empty() {
        response.insert(
            "errors".into(),
            serde_json::to_value(errors).expect("errors serialize"),
        );
    }
    ReferenceOutput {
        response: serde_json::Value::Object(response),
        calls,
    }
}
