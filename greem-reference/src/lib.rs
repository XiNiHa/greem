//! The depth-first reference executor: one field at a time, one object at a
//! time, building `serde_json::Value` directly and bubbling nulls the spec's
//! way. It shares only the execution tree and the planning pass with the
//! breadth-first executor and exists to prove that executor equivalent.

use greem::__private::SchemaInfo;
use greem::__private::{RefCompletion, RefShared, response};
use greem::{
    ExecuteOptions, IncrementalDelivery, Operation, OperationKind, Outputs, Roots, Schema, Shape,
};
use std::sync::{Arc, Mutex};

/// The non-incremental responses plus the number of resolver invocations:
/// one response for a query or mutation, one per event for a subscription.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceOutput {
    pub responses: Vec<serde_json::Value>,
    pub calls: u64,
}

impl ReferenceOutput {
    /// The one response of a query or mutation.
    pub fn response(&self) -> &serde_json::Value {
        assert_eq!(
            self.responses.len(),
            1,
            "one response: {:?}",
            self.responses
        );
        &self.responses[0]
    }
}

/// Executes `op` depth-first over the same Plan table the breadth-first
/// executor would build. `@defer` and `@stream` are ignored. A subscription
/// drains its event source and executes every event in turn.
pub async fn execute<I, C, Q, M, S>(
    schema: &Schema<I, C, Q, M, S>,
    roots: Roots<Q, M, S>,
    ctx: C,
    op: Operation,
    options: ExecuteOptions,
) -> ReferenceOutput
where
    I: SchemaInfo,
    C: Send + Sync + 'static,
    Q: Outputs<I::Query, C> + Send + Sync,
    M: Outputs<I::Mutation, C> + Send + Sync,
    S: Outputs<I::Subscription, C> + Send + Sync,
{
    let app = Arc::new(ctx);
    let planned = schema.plan(
        &op,
        &app,
        ExecuteOptions {
            incremental: IncrementalDelivery::Disabled,
            ..options
        },
    );
    let (table, introspection, kind, behavior) = match planned {
        Ok(plan) => plan.__parts(),
        Err(errors) => {
            return ReferenceOutput {
                responses: vec![serde_json::json!({ "errors": errors.errors })],
                calls: 0,
            };
        }
    };
    let shared = RefShared {
        table,
        app,
        behavior,
        introspection,
        errors: Mutex::new(Vec::new()),
        calls: Mutex::new(0),
        serial_root: kind == OperationKind::Mutation,
        responses: Mutex::new(Vec::new()),
    };
    let rc = RefCompletion::new(&shared, 0, Vec::new(), Shape::new(&[], false), Vec::new());
    let Roots {
        query,
        mutation,
        subscription,
    } = roots;
    // A failed subscription root is `data: null` at the root, like any
    // failed root; the subscribe step never runs.
    let failed_root = match kind {
        OperationKind::Subscription => subscription.__parent_error().cloned(),
        _ => None,
    };
    let data = match (kind, &failed_root) {
        (OperationKind::Query, _) => <Q as Outputs<I::Query, C>>::__reference(query, &rc).await,
        (OperationKind::Mutation, _) => {
            <M as Outputs<I::Mutation, C>>::__reference(mutation, &rc).await
        }
        (OperationKind::Subscription, Some(error)) => rc.error(error.clone()).await,
        (OperationKind::Subscription, None) => {
            <S as Outputs<I::Subscription, C>>::__reference(subscription, &rc).await
        }
    };
    let errors = std::mem::take(&mut *shared.errors.lock().unwrap());
    let calls = *shared.calls.lock().unwrap();
    let responses = match kind {
        OperationKind::Query | OperationKind::Mutation => vec![response(
            data.unwrap_or(serde_json::Value::Null),
            errors,
            behavior,
        )],
        OperationKind::Subscription if failed_root.is_some() => {
            vec![response(serde_json::Value::Null, errors, behavior)]
        }
        // Every event pushed its own response; an error left over is the
        // subscribe step's, the subscription's one response.
        OperationKind::Subscription => {
            let mut responses = std::mem::take(&mut *shared.responses.lock().unwrap());
            if !errors.is_empty() {
                responses.push(serde_json::json!({ "errors": errors }));
            }
            responses
        }
    };
    ReferenceOutput { responses, calls }
}
