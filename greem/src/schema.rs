//! `Schema<I, C, Q, M>`: the runtime handle built once from the generated
//! schema module, with the request entry points.

use crate::context::Context;
use crate::error::{GraphQLError, InputError, RequestErrorKind, SchemaError};
use crate::exec::complete::root_batch;
use crate::exec::payload::{Payload, PayloadKind};
use crate::exec::pull::{Ship, pull};
use crate::exec::run::run_loop;
use crate::exec::scope::Batch;
use crate::exec::state::{ErrorBehavior, ExecuteOptions, IncrementalDelivery, Shared};
use crate::exec::subscription::EventEnv;
use crate::plan::{Leaf, PlanTable, Walker};
use crate::resolver::{NoMutation, NoSubscription, Outputs};
use crate::tree::{Abort, Document, Tree, json_to_input, span_location};
use crate::value::InputValue;
use apollo_compiler::validation::Valid;
use apollo_compiler::{ExecutableDocument, Schema as ApolloSchema};
use futures::{Stream, StreamExt};
use std::marker::PhantomData;
use std::sync::Arc;

/// Generated per schema module: the embedded SDL and the root type tags.
pub trait SchemaInfo: Send + Sync + 'static {
    const SDL: &'static str;
    const BUILD_VERSION: &'static str;
    type Query;
    type Mutation;
    type Subscription;

    /// Runs the codec of the custom scalar `name` over an input value, so a
    /// variable is validated before execution; other names accept anything.
    fn check_scalar(_name: &str, _value: &InputValue) -> Result<(), InputError> {
        Ok(())
    }
}

/// The root values for one request.
pub struct Roots<Q, M = NoMutation, S = NoSubscription> {
    pub query: Q,
    pub mutation: M,
    pub subscription: S,
}

impl<Q> Roots<Q, NoMutation, NoSubscription> {
    pub fn query(query: Q) -> Self {
        Roots {
            query,
            mutation: NoMutation,
            subscription: NoSubscription,
        }
    }
}

impl<Q, M, S> Roots<Q, M, S> {
    pub fn with_mutation<M2>(self, mutation: M2) -> Roots<Q, M2, S> {
        Roots {
            query: self.query,
            mutation,
            subscription: self.subscription,
        }
    }

    pub fn with_subscription<S2>(self, subscription: S2) -> Roots<Q, M, S2> {
        Roots {
            query: self.query,
            mutation: self.mutation,
            subscription,
        }
    }
}

/// The kind of the operation a request executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Query,
    Mutation,
    Subscription,
}

/// A handle to a built schema; clones share it.
pub struct Schema<I: SchemaInfo, C = (), Q = (), M = NoMutation, S = NoSubscription> {
    inner: Arc<Inner>,
    _marker: Marker<I, C, Q, M, S>,
}

struct Inner {
    schema: Arc<Valid<ApolloSchema>>,
    implementers: apollo_compiler::collections::HashMap<
        apollo_compiler::Name,
        apollo_compiler::schema::Implementers,
    >,
    max_depth: u32,
    stream_capacity: usize,
    recursion_limit: Option<usize>,
    token_limit: Option<usize>,
}

impl<I: SchemaInfo, C, Q, M, S> Clone for Schema<I, C, Q, M, S> {
    fn clone(&self) -> Self {
        Schema {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

type Marker<I, C, Q, M, S> = PhantomData<fn() -> (I, C, Q, M, S)>;

pub struct SchemaBuilder<I: SchemaInfo, C, Q, M, S = NoSubscription> {
    max_depth: u32,
    stream_capacity: usize,
    recursion_limit: Option<usize>,
    token_limit: Option<usize>,
    _marker: Marker<I, C, Q, M, S>,
}

impl<I: SchemaInfo, C: Send + Sync + 'static> Schema<I, C, (), NoMutation, NoSubscription> {
    pub fn builder() -> SchemaBuilder<I, C, (), NoMutation, NoSubscription> {
        SchemaBuilder {
            max_depth: 32,
            stream_capacity: 100,
            recursion_limit: None,
            token_limit: None,
            _marker: PhantomData,
        }
    }
}

impl<I: SchemaInfo, C: Send + Sync + 'static, Q, M, S> SchemaBuilder<I, C, Q, M, S> {
    pub fn query<Q2: Outputs<I::Query, C>>(self) -> SchemaBuilder<I, C, Q2, M, S> {
        SchemaBuilder {
            max_depth: self.max_depth,
            stream_capacity: self.stream_capacity,
            recursion_limit: self.recursion_limit,
            token_limit: self.token_limit,
            _marker: PhantomData,
        }
    }

    pub fn mutation<M2: Outputs<I::Mutation, C>>(self) -> SchemaBuilder<I, C, Q, M2, S> {
        SchemaBuilder {
            max_depth: self.max_depth,
            stream_capacity: self.stream_capacity,
            recursion_limit: self.recursion_limit,
            token_limit: self.token_limit,
            _marker: PhantomData,
        }
    }

    pub fn subscription<S2: Outputs<I::Subscription, C>>(self) -> SchemaBuilder<I, C, Q, M, S2> {
        SchemaBuilder {
            max_depth: self.max_depth,
            stream_capacity: self.stream_capacity,
            recursion_limit: self.recursion_limit,
            token_limit: self.token_limit,
            _marker: PhantomData,
        }
    }

    /// The maximum execution depth (default 32); see the depth policy in the spec.
    pub fn max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    /// Items buffered per stream turn (default 100).
    pub fn stream_capacity(mut self, capacity: usize) -> Self {
        self.stream_capacity = capacity.max(1);
        self
    }

    pub fn parser_recursion_limit(mut self, limit: usize) -> Self {
        self.recursion_limit = Some(limit);
        self
    }

    pub fn parser_token_limit(mut self, limit: usize) -> Self {
        self.token_limit = Some(limit);
        self
    }

    /// Every root must be configured for what the schema declares: a
    /// declared `Mutation` without `.mutation::<M>()`, or a declared
    /// `Subscription` without `.subscription::<S>()`, does not compile.
    pub fn build(self) -> Result<Schema<I, C, Q, M, S>, SchemaError>
    where
        Q: Outputs<I::Query, C>,
        M: Outputs<I::Mutation, C>,
        S: Outputs<I::Subscription, C>,
    {
        if I::BUILD_VERSION != crate::BUILD_VERSION {
            return Err(SchemaError(format!(
                "schema module was generated by greem-build {} but greem is {}; rebuild with matching versions",
                I::BUILD_VERSION,
                crate::BUILD_VERSION
            )));
        }
        let schema = ApolloSchema::parse_and_validate(I::SDL, "schema.graphql")
            .map_err(|e| SchemaError(e.to_string()))?;
        let implementers = schema.implementers_map();
        Ok(Schema {
            inner: Arc::new(Inner {
                schema: Arc::new(schema),
                implementers,
                max_depth: self.max_depth,
                stream_capacity: self.stream_capacity,
                recursion_limit: self.recursion_limit,
                token_limit: self.token_limit,
            }),
            _marker: PhantomData,
        })
    }
}

/// A request-level failure: the step that failed and its errors.
#[derive(Debug, Clone)]
pub struct RequestErrors {
    pub kind: RequestErrorKind,
    pub errors: Vec<GraphQLError>,
}

impl RequestErrors {
    pub(crate) fn one(
        kind: RequestErrorKind,
        message: impl Into<String>,
        locations: Vec<crate::error::Location>,
    ) -> Self {
        RequestErrors {
            kind,
            errors: vec![GraphQLError::request(message, locations)],
        }
    }

    pub fn into_payload(self) -> OwnedPayload {
        OwnedPayload::encode(self.payload())
    }

    pub(crate) fn payload(self) -> Payload<'static> {
        Payload::request_error(self.kind, self.errors)
    }
}

/// One operation of a parsed document plus its variables and the error
/// behavior the request asked for, if any.
pub struct Operation {
    pub document: Arc<Document>,
    pub operation_name: Option<String>,
    pub variables: serde_json::Map<String, serde_json::Value>,
    pub on_error: Option<ErrorBehavior>,
}

impl Operation {
    pub fn new(document: Arc<Document>) -> Self {
        Operation {
            document,
            operation_name: None,
            variables: Default::default(),
            on_error: None,
        }
    }

    pub fn with_operation_name(mut self, name: impl Into<String>) -> Self {
        self.operation_name = Some(name.into());
        self
    }

    pub fn with_variables(mut self, variables: serde_json::Map<String, serde_json::Value>) -> Self {
        self.variables = variables;
        self
    }

    pub fn with_on_error(mut self, on_error: ErrorBehavior) -> Self {
        self.on_error = Some(on_error);
        self
    }

    /// The error behavior this operation runs under: the request's `onError`,
    /// else `NULL` when the executed operation carries graphql-js's
    /// `@experimental_disableErrorPropagation` (which the schema has to
    /// declare for the document to validate), else the server's default.
    fn error_behavior(
        &self,
        operation: &apollo_compiler::executable::Operation,
        options: ExecuteOptions,
    ) -> ErrorBehavior {
        self.on_error
            .or_else(|| {
                operation
                    .directives
                    .has("experimental_disableErrorPropagation")
                    .then_some(ErrorBehavior::Null)
            })
            .unwrap_or(options.default_error_behavior)
    }
    /// Whether the operation to execute is a mutation; false when it cannot
    /// be determined, which execution reports as a request error.
    pub(crate) fn is_mutation(&self) -> bool {
        self.document
            .doc
            .operations
            .get(self.operation_name.as_deref())
            .is_ok_and(|operation| operation.is_mutation())
    }

    /// Whether the operation to execute is a subscription; false when it
    /// cannot be determined.
    pub(crate) fn is_subscription(&self) -> bool {
        self.document
            .doc
            .operations
            .get(self.operation_name.as_deref())
            .is_ok_and(|operation| operation.is_subscription())
    }
}

/// Lookbehind planning's output for one request: the frozen Plan table, the
/// introspection result and the operation kind, bound to the options it was
/// planned under. Hints read the request context during planning, so a plan
/// belongs to the request it was made for and is never reused across
/// requests. Its typed entries are the generated plan types of one schema,
/// so a plan runs only on the `Schema` value that made it.
pub struct Plan<I: SchemaInfo, C = (), Q = (), M = NoMutation, S = NoSubscription> {
    schema: Arc<Inner>,
    table: Arc<PlanTable>,
    introspection: Option<serde_json::Value>,
    kind: OperationKind,
    /// Resolved from the operation and the options; see
    /// `Operation::error_behavior`.
    error_behavior: ErrorBehavior,
    options: ExecuteOptions,
    _marker: Marker<I, C, Q, M, S>,
}

impl<I: SchemaInfo, C, Q, M, S> Plan<I, C, Q, M, S> {
    /// The error behavior the plan runs under.
    pub fn error_behavior(&self) -> ErrorBehavior {
        self.error_behavior
    }

    /// The kind of operation the plan executes: a subscription's stream has
    /// one complete response per event, so a transport can tell before the
    /// first payload.
    pub fn kind(&self) -> OperationKind {
        self.kind
    }

    /// The reference executor's view: the table, the introspection JSON, the
    /// operation kind and the error behavior.
    #[doc(hidden)]
    #[cfg(feature = "reference-executor")]
    pub fn __parts(
        self,
    ) -> (
        Arc<PlanTable>,
        Option<serde_json::Value>,
        OperationKind,
        ErrorBehavior,
    ) {
        (
            self.table,
            self.introspection,
            self.kind,
            self.error_behavior,
        )
    }
}

impl<I: SchemaInfo, C, Q, M, S> std::fmt::Debug for Plan<I, C, Q, M, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("kind", &self.kind)
            .field("error_behavior", &self.error_behavior)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// A serialized payload owned by the caller.
#[derive(Clone, Debug)]
pub struct OwnedPayload {
    pub kind: PayloadKind,
    pub has_next: Option<bool>,
    pub json: Vec<u8>,
}

impl OwnedPayload {
    /// The JSON encoder, for `Schema::execute_stream`.
    pub fn encode(payload: Payload<'_>) -> Self {
        OwnedPayload {
            kind: payload.kind(),
            has_next: payload.has_next(),
            json: serde_json::to_vec(&payload).expect("serialize payload"),
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.json).expect("payload JSON is UTF-8")
    }
}

/// Every payload of one execution, serialized.
#[derive(Clone, Debug, Default)]
pub struct ExecutionOutput {
    pub payloads: Vec<OwnedPayload>,
}

impl ExecutionOutput {
    pub fn is_incremental(&self) -> bool {
        self.payloads.first().is_some_and(|p| p.has_next.is_some())
    }

    /// The initial payload, or the request error. A query or mutation that
    /// runs to completion always has one; a subscription whose event source
    /// ends without an event has none.
    pub fn first(&self) -> Option<&OwnedPayload> {
        self.payloads.first()
    }
}

impl<I: SchemaInfo, C: Send + Sync + 'static, Q, M, S> Schema<I, C, Q, M, S> {
    /// The schema this runtime serves: the merged SDL from greem-build, with
    /// any `@defer` and `@stream` definitions it supplied.
    pub fn sdl(&self) -> &'static str {
        I::SDL
    }

    pub fn max_depth(&self) -> u32 {
        self.inner.max_depth
    }

    /// Parses and validates a request document once; the result is shared
    /// between requests.
    pub fn parse(&self, source: &str) -> Result<Arc<Document>, RequestErrors> {
        let mut parser = apollo_compiler::parser::Parser::new();
        if let Some(limit) = self.inner.recursion_limit {
            parser = parser.recursion_limit(limit);
        }
        if let Some(limit) = self.inner.token_limit {
            parser = parser.token_limit(limit);
        }
        let ast = parser
            .parse_ast(source, "request.graphql")
            .map_err(|e| RequestErrors {
                kind: RequestErrorKind::Parse,
                errors: diagnostics_to_errors(&e.errors),
            })?;
        let doc = ast
            .to_executable_validate(&self.inner.schema)
            .map_err(|e| RequestErrors {
                kind: RequestErrorKind::Validation,
                errors: diagnostics_to_errors(&e.errors),
            })?;
        let overlaps = crate::tree::overlapping_streams(&doc, &self.inner.schema);
        if !overlaps.is_empty() {
            return Err(RequestErrors {
                kind: RequestErrorKind::Validation,
                errors: overlaps,
            });
        }
        Ok(Arc::new(Document { doc }))
    }

    fn prepare(&self, op: &Operation, options: ExecuteOptions) -> Result<Tree, RequestErrors> {
        let doc: &Valid<ExecutableDocument> = &op.document.doc;
        let operation = doc
            .operations
            .get(op.operation_name.as_deref())
            .map_err(|e| {
                RequestErrors::one(
                    RequestErrorKind::Operation,
                    e.message().to_string(),
                    Vec::new(),
                )
            })?
            .clone();
        let variables: apollo_compiler::response::JsonMap = op
            .variables
            .iter()
            .map(|(k, v)| {
                (
                    apollo_compiler::response::serde_json_bytes::ByteString::from(k.as_str()),
                    apollo_compiler::response::JsonValue::from(v.clone()),
                )
            })
            .collect();
        let coerced = apollo_compiler::request::coerce_variable_values(
            &self.inner.schema,
            &operation,
            &variables,
        )
        .map_err(|e| RequestErrors {
            kind: RequestErrorKind::Variables,
            errors: vec![request_error_to_graphql(&e, &doc.sources)],
        })?;
        // apollo accepts any value for a custom scalar: its codec runs here,
        // so an invalid variable is a request error and nothing executes.
        for variable in &operation.variables {
            let Some(value) = coerced.get(variable.name.as_str()) else {
                continue;
            };
            check_scalars::<I>(&self.inner.schema, &variable.ty, value).map_err(|e| {
                RequestErrors::one(
                    RequestErrorKind::Variables,
                    format!("variable `${}` got an invalid value: {e}", variable.name),
                    span_location(variable.location(), doc),
                )
            })?;
        }
        apollo_compiler::introspection::check_max_depth(doc, &operation).map_err(|e| {
            RequestErrors {
                kind: RequestErrorKind::Validation,
                errors: vec![request_error_to_graphql(&e, &doc.sources)],
            }
        })?;
        Ok(Tree::new(
            self.inner.schema.clone(),
            op.document.clone(),
            operation,
            coerced,
            options.incremental == IncrementalDelivery::Enabled,
            self.inner.max_depth,
        ))
    }

    fn introspection(&self, tree: &Tree) -> Result<Option<serde_json::Value>, RequestErrors> {
        let doc: &Valid<ExecutableDocument> = &tree.doc.doc;
        let has_introspection = tree
            .operation
            .root_fields(doc)
            .any(|f| matches!(f.name.as_str(), "__schema" | "__type"));
        if !has_introspection {
            return Ok(None);
        }
        let response = apollo_compiler::introspection::partial_execute(
            &self.inner.schema,
            &self.inner.implementers,
            doc,
            &tree.operation,
            &tree.variables,
        )
        .map_err(|e| RequestErrors {
            kind: RequestErrorKind::Validation,
            errors: vec![request_error_to_graphql(&e, &doc.sources)],
        })?;
        let data = serde_json::to_value(response.data).unwrap_or(serde_json::Value::Null);
        Ok(Some(data))
    }

    /// Plans one request: coerces its variables, builds the execution tree
    /// and runs lookbehind planning over it with `ctx` as the request
    /// context, before any resolver runs. A request error here is the whole
    /// response; `execute_plan_stream` runs the result.
    pub fn plan(
        &self,
        op: &Operation,
        ctx: &C,
        options: ExecuteOptions,
    ) -> Result<Plan<I, C, Q, M, S>, RequestErrors>
    where
        Q: Outputs<I::Query, C>,
        M: Outputs<I::Mutation, C>,
        S: Outputs<I::Subscription, C>,
    {
        let mut tree = self.prepare(op, options)?;
        let error_behavior = op.error_behavior(&tree.operation, options);
        let introspection = self.introspection(&tree)?;
        let planning_ctx = Context::new(ctx, None);
        let mut table = PlanTable::default();
        let kind = tree.kind();
        let walk = {
            let mut walker = Walker::new(&mut tree, &mut table, &planning_ctx);
            match kind {
                OperationKind::Query => Q::__walk(&mut walker, 0, &Leaf::default()),
                OperationKind::Mutation => M::__walk(&mut walker, 0, &Leaf::default()),
                OperationKind::Subscription => S::__walk(&mut walker, 0, &Leaf::default()),
            }
        };
        walk.map_err(|Abort(kind, errors)| RequestErrors { kind, errors })?;
        // Validation leaves a subscription exactly one root field, with no
        // `@skip`/`@include` that could exclude it; the subscribe step
        // indexes it, so the invariant is checked rather than assumed.
        if kind == OperationKind::Subscription && table.header(0).sets[0].1.len() != 1 {
            return Err(RequestErrors::one(
                RequestErrorKind::Validation,
                "a subscription must select exactly one root field",
                Vec::new(),
            ));
        }
        table.usages = tree.usages.clone();
        table.compute_beneath();
        Ok(Plan {
            schema: self.inner.clone(),
            table: Arc::new(table),
            introspection,
            kind,
            error_behavior,
            options,
            _marker: PhantomData,
        })
    }

    /// Plans and executes one request as a stream of payloads, each passed
    /// to `encode` while the executor's frames are alive; pass
    /// `OwnedPayload::encode` for owned JSON. Execution advances only while
    /// the stream is polled and pauses after each payload until the next
    /// poll. Dropping the stream cancels it, a mutation's root fields
    /// included.
    pub fn execute_stream<T, E>(
        &self,
        roots: Roots<Q, M, S>,
        ctx: C,
        op: Operation,
        options: ExecuteOptions,
        encode: E,
    ) -> impl Stream<Item = T> + Send + use<I, C, Q, M, S, T, E>
    where
        Q: Outputs<I::Query, C> + Send + Sync,
        M: Outputs<I::Mutation, C> + Send + Sync,
        S: Outputs<I::Subscription, C> + Send + Sync,
        T: Send,
        E: for<'p> FnMut(Payload<'p>) -> T + Send,
    {
        let schema = self.clone();
        pull(encode, move |mut outlet| async move {
            match schema.plan(&op, &ctx, options) {
                Ok(plan) => schema.run(roots, ctx, plan, &mut outlet).await,
                Err(errors) => outlet.ship(errors.payload()),
            }
        })
    }

    /// Executes a plan this schema made with `plan` and the same `ctx`,
    /// streaming like `execute_stream`.
    ///
    /// # Panics
    ///
    /// If the plan was made by another `Schema` value, whose depth and
    /// stream limits it was checked against.
    pub fn execute_plan_stream<T, E>(
        &self,
        roots: Roots<Q, M, S>,
        ctx: C,
        plan: Plan<I, C, Q, M, S>,
        encode: E,
    ) -> impl Stream<Item = T> + Send + use<I, C, Q, M, S, T, E>
    where
        Q: Outputs<I::Query, C> + Send + Sync,
        M: Outputs<I::Mutation, C> + Send + Sync,
        S: Outputs<I::Subscription, C> + Send + Sync,
        T: Send,
        E: for<'p> FnMut(Payload<'p>) -> T + Send,
    {
        assert!(
            Arc::ptr_eq(&self.inner, &plan.schema),
            "a Plan runs only on the Schema that made it"
        );
        let schema = self.clone();
        pull(encode, move |mut outlet| async move {
            schema.run(roots, ctx, plan, &mut outlet).await
        })
    }

    /// The text path: parses and validates a whole request, then streams it
    /// like `execute_stream`.
    pub fn execute_request_stream<T, E>(
        &self,
        roots: Roots<Q, M, S>,
        ctx: C,
        request: crate::http::Request,
        options: ExecuteOptions,
        encode: E,
    ) -> impl Stream<Item = T> + Send + use<I, C, Q, M, S, T, E>
    where
        Q: Outputs<I::Query, C> + Send + Sync,
        M: Outputs<I::Mutation, C> + Send + Sync,
        S: Outputs<I::Subscription, C> + Send + Sync,
        T: Send,
        E: for<'p> FnMut(Payload<'p>) -> T + Send,
    {
        let schema = self.clone();
        pull(encode, move |mut outlet| async move {
            let planned = schema
                .operation(request)
                .and_then(|op| schema.plan(&op, &ctx, options));
            match planned {
                Ok(plan) => schema.run(roots, ctx, plan, &mut outlet).await,
                Err(errors) => outlet.ship(errors.payload()),
            }
        })
    }

    /// Executes and returns every payload as owned JSON.
    pub async fn execute(
        &self,
        roots: Roots<Q, M, S>,
        ctx: C,
        op: Operation,
        options: ExecuteOptions,
    ) -> ExecutionOutput
    where
        Q: Outputs<I::Query, C> + Send + Sync,
        M: Outputs<I::Mutation, C> + Send + Sync,
        S: Outputs<I::Subscription, C> + Send + Sync,
    {
        ExecutionOutput {
            payloads: self
                .execute_stream(roots, ctx, op, options, OwnedPayload::encode)
                .collect()
                .await,
        }
    }

    /// The text path, returning every payload as owned JSON.
    pub async fn execute_request(
        &self,
        roots: Roots<Q, M, S>,
        ctx: C,
        request: crate::http::Request,
        options: ExecuteOptions,
    ) -> ExecutionOutput
    where
        Q: Outputs<I::Query, C> + Send + Sync,
        M: Outputs<I::Mutation, C> + Send + Sync,
        S: Outputs<I::Subscription, C> + Send + Sync,
    {
        ExecutionOutput {
            payloads: self
                .execute_request_stream(roots, ctx, request, options, OwnedPayload::encode)
                .collect()
                .await,
        }
    }

    pub(crate) fn operation(
        &self,
        request: crate::http::Request,
    ) -> Result<Operation, RequestErrors> {
        let crate::http::Request {
            query,
            operation_name,
            variables,
            on_error,
            ..
        } = request;
        Ok(Operation {
            document: self.parse(&query)?,
            operation_name,
            variables: variables.unwrap_or_default(),
            on_error,
        })
    }

    async fn run(
        &self,
        roots: Roots<Q, M, S>,
        ctx: C,
        plan: Plan<I, C, Q, M, S>,
        outlet: &mut dyn Ship,
    ) where
        Q: Outputs<I::Query, C> + Send + Sync,
        M: Outputs<I::Mutation, C> + Send + Sync,
        S: Outputs<I::Subscription, C> + Send + Sync,
    {
        let Plan {
            table,
            introspection,
            kind,
            error_behavior,
            options,
            ..
        } = plan;
        let capacity = self.inner.stream_capacity;
        let Roots {
            query,
            mutation,
            subscription,
        } = roots;
        // A failed root is marked once, at its own position, like any object.
        let failed = match kind {
            OperationKind::Query => query.__parent_error(),
            OperationKind::Mutation => mutation.__parent_error(),
            OperationKind::Subscription => subscription.__parent_error(),
        };
        if let Some(error) = failed {
            outlet.ship(Payload::failed_root(GraphQLError::from_error(
                error,
                Vec::new(),
                Vec::new(),
            )));
            return;
        }
        let incremental = options.incremental == IncrementalDelivery::Enabled;
        match kind {
            OperationKind::Query => {
                let shared = Shared::new(
                    table.clone(),
                    error_behavior,
                    incremental,
                    capacity,
                    introspection,
                );
                let batch = root_batch::<Q, I::Query, C>(query, &shared, &ctx, &table, false);
                let mut root = batch.start();
                run_loop(&mut root, &shared, outlet).await;
            }
            OperationKind::Mutation => {
                let shared = Shared::new(
                    table.clone(),
                    error_behavior,
                    incremental,
                    capacity,
                    introspection,
                );
                let batch = root_batch::<M, I::Mutation, C>(mutation, &shared, &ctx, &table, true);
                let mut root = batch.start();
                run_loop(&mut root, &shared, outlet).await;
            }
            OperationKind::Subscription => {
                subscribe_and_run::<S, I::Subscription, C>(
                    subscription,
                    &ctx,
                    &table,
                    error_behavior,
                    capacity,
                    outlet,
                )
                .await;
            }
        }
    }
}

/// Runs a subscription: the subscribe step once, then one execution per
/// event, each polled from the event source only once the consumer has taken
/// the previous response. Incremental delivery is off: a subscription cannot
/// defer or stream, so every event is one complete response.
async fn subscribe_and_run<S, Ty, C>(
    root: S,
    app: &C,
    table: &Arc<PlanTable>,
    behavior: ErrorBehavior,
    capacity: usize,
    outlet: &mut dyn Ship,
) where
    S: Outputs<Ty, C> + Send + Sync,
    Ty: 'static,
    C: Send + Sync,
{
    let shared = Shared::new(table.clone(), behavior, false, capacity, None);
    let batch = root_batch::<S, Ty, C>(root, &shared, app, table, false);
    let mut events = match batch.subscribe().await {
        Ok(events) => events,
        Err(error) => {
            let header = table.header(0);
            let field = &header.fields[header.sets[0].1[0] as usize];
            outlet.ship(Payload::subscribe_failed(GraphQLError::from_error(
                &error,
                field.spans.clone(),
                vec![crate::error::PathSegment::Key(field.key.clone())],
            )));
            return;
        }
    };
    loop {
        futures::future::poll_fn(|_| outlet.poll_taken()).await;
        let shared = Shared::new(table.clone(), behavior, false, capacity, None);
        let env = EventEnv {
            shared: &shared,
            app,
            table,
        };
        if !events.next_event(env, outlet).await {
            break;
        }
    }
}

/// Validates every custom scalar inside a coerced variable value of type `ty`.
fn check_scalars<I: SchemaInfo>(
    schema: &ApolloSchema,
    ty: &apollo_compiler::ast::Type,
    value: &apollo_compiler::response::JsonValue,
) -> Result<(), InputError> {
    use apollo_compiler::response::JsonValue;
    use apollo_compiler::schema::ExtendedType;
    if value.is_null() {
        return Ok(());
    }
    if ty.is_list() {
        let item = ty.item_type();
        return match value {
            JsonValue::Array(items) => items.iter().enumerate().try_for_each(|(i, value)| {
                check_scalars::<I>(schema, item, value).map_err(|e| e.at(&i.to_string()))
            }),
            single => check_scalars::<I>(schema, item, single),
        };
    }
    let name = ty.inner_named_type();
    match schema.types.get(name) {
        Some(ExtendedType::Scalar(scalar)) if !scalar.is_built_in() => {
            I::check_scalar(name.as_str(), &json_to_input(schema, Some(ty), value))
        }
        Some(ExtendedType::InputObject(object)) => match value {
            JsonValue::Object(fields) => {
                object.fields.iter().try_for_each(|(field, definition)| {
                    match fields.get(field.as_str()) {
                        Some(value) => check_scalars::<I>(schema, &definition.ty, value)
                            .map_err(|e| e.at(field.as_str())),
                        None => Ok(()),
                    }
                })
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

fn diagnostics_to_errors(list: &apollo_compiler::validation::DiagnosticList) -> Vec<GraphQLError> {
    list.iter()
        .map(|d| {
            let locations = d
                .line_column_range()
                .map(|r| {
                    vec![crate::error::Location {
                        line: r.start.line,
                        column: r.start.column,
                    }]
                })
                .unwrap_or_default();
            GraphQLError::request(d.error.to_string(), locations)
        })
        .collect()
}

fn request_error_to_graphql(
    error: &apollo_compiler::request::RequestError,
    sources: &apollo_compiler::parser::SourceMap,
) -> GraphQLError {
    let locations = error
        .location()
        .and_then(|span| span.line_column(sources))
        .map(|lc| {
            vec![crate::error::Location {
                line: lc.line,
                column: lc.column,
            }]
        })
        .unwrap_or_default();
    GraphQLError::request(error.message().to_string(), locations)
}
