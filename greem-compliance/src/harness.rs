//! The harness: the executor-facing controls every compliance world embeds
//! (failure map, interleaving yield counts, call log, gate), and the run
//! helpers that drive both executors over any suite's schema.

use futures::executor::block_on;
use greem::__private::SchemaInfo;
use greem::{
    ExecuteOptions, IncrementalDelivery, Operation, Outputs, Roots, Schema, SchemaBuilder,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Failure {
    pub type_name: &'static str,
    pub field: &'static str,
    pub object: u32,
}

/// The call log: per resolver call, the field's `Type.field` name and its parent count.
pub type Calls = Vec<(&'static str, usize)>;

#[derive(Debug, Default)]
pub struct Harness {
    pub failures: BTreeSet<Failure>,
    /// The interleaving: yield counts consumed per resolver call, cyclically.
    pub yields: Vec<u32>,
    pub calls: Arc<Mutex<Calls>>,
    /// A field whose resolver stays pending until `open_gate` is called.
    pub gate_field: Option<&'static str>,
    pub gate_open: AtomicBool,
    /// A field whose resolver panics.
    pub panic_field: Option<&'static str>,
    /// Records drops of tracked objects when set.
    pub track_drops: bool,
}

/// The same configuration with a fresh call log, for a second run.
impl Clone for Harness {
    fn clone(&self) -> Self {
        Harness {
            failures: self.failures.clone(),
            yields: self.yields.clone(),
            calls: Arc::default(),
            gate_field: self.gate_field,
            gate_open: AtomicBool::new(self.gate_open.load(Ordering::SeqCst)),
            panic_field: self.panic_field,
            track_drops: self.track_drops,
        }
    }
}

impl Harness {
    pub fn failing(failures: &[(&'static str, &'static str, u32)]) -> Self {
        Harness {
            failures: failures
                .iter()
                .map(|&(type_name, field, object)| Failure {
                    type_name,
                    field,
                    object,
                })
                .collect(),
            ..Default::default()
        }
    }

    pub fn fails(&self, type_name: &'static str, field: &'static str, object: u32) -> bool {
        self.failures.contains(&Failure {
            type_name,
            field,
            object,
        })
    }

    /// `value`, or the failure-map error for this position.
    pub fn fail_or<T>(
        &self,
        ty: &'static str,
        field: &'static str,
        object: u32,
        value: T,
    ) -> Result<T, greem::Error> {
        if self.fails(ty, field, object) {
            Err(greem::Error::new(format!(
                "{ty}.{field} failed for {object}"
            )))
        } else {
            Ok(value)
        }
    }

    pub fn call(&self, name: &'static str, parents: usize) {
        self.calls.lock().unwrap().push((name, parents));
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    pub fn open_gate(&self) {
        self.gate_open.store(true, Ordering::SeqCst);
    }

    /// Logs one resolver call and runs its interleaving: the yields for this
    /// call, then the panic or the gate if `name` is the configured field.
    pub async fn trace(&self, name: &'static str, parents: usize) {
        self.call(name, parents);
        self.pause(name).await;
    }

    pub async fn pause(&self, name: &'static str) {
        if self.panic_field == Some(name) {
            panic!("intentional panic in {name}");
        }
        let yields = if self.yields.is_empty() {
            0
        } else {
            let n = self.calls.lock().unwrap().len();
            self.yields[n % self.yields.len()]
        };
        for _ in 0..yields {
            pending_once().await;
        }
        if self.gate_field == Some(name) {
            std::future::poll_fn(|_| {
                if self.gate_open.load(Ordering::SeqCst) {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            })
            .await;
        }
    }

    /// True when a run of this world gates or panics on purpose, so a second
    /// run against the reference executor is not safe.
    pub fn is_interactive(&self) -> bool {
        self.gate_field.is_some() || self.panic_field.is_some()
    }
}

/// Returns Pending once and wakes itself.
pub async fn pending_once() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}

/// A world's harness.
pub trait HasHarness {
    fn harness(&self) -> &Harness;
}

/// A compliance world with the area schema and roots it runs under. The run
/// helpers hang off it so every area drives both executors the same way.
pub trait Area: HasHarness + Clone + Send + Sync + Sized + 'static {
    type Info: SchemaInfo;
    type Query: Outputs<<Self::Info as SchemaInfo>::Query, Self> + Send + Sync;
    type Mutation: Outputs<<Self::Info as SchemaInfo>::Mutation, Self> + Send + Sync;

    /// The schema builder with the roots set; [`Area::schema`] finishes it.
    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation>;
    fn roots() -> Roots<Self::Query, Self::Mutation>;

    /// The schema, with the items buffered per stream turn set when given.
    fn schema(
        stream_capacity: Option<usize>,
    ) -> Schema<Self::Info, Self, Self::Query, Self::Mutation> {
        let builder = Self::builder();
        match stream_capacity {
            Some(capacity) => builder.stream_capacity(capacity),
            None => builder,
        }
        .build()
        .expect("area schema builds")
    }

    /// Runs the BFS executor; returns every payload as JSON and the call log.
    fn run(self, query: &str, variables: Value, options: ExecuteOptions) -> (Vec<Value>, Calls) {
        self.run_operation(None, query, variables, options, None)
    }

    /// [`Area::run`] with the items buffered per stream turn set (`None`: the default).
    fn run_at_capacity(
        self,
        query: &str,
        variables: Value,
        options: ExecuteOptions,
        stream_capacity: Option<usize>,
    ) -> (Vec<Value>, Calls) {
        self.run_operation(None, query, variables, options, stream_capacity)
    }

    /// [`Area::run`] of the named operation of a multi-operation document.
    fn run_operation(
        self,
        operation_name: Option<&str>,
        query: &str,
        variables: Value,
        options: ExecuteOptions,
        stream_capacity: Option<usize>,
    ) -> (Vec<Value>, Calls) {
        let schema = Self::schema(stream_capacity);
        let document = match schema.parse(query) {
            Ok(d) => d,
            Err(e) => {
                return (
                    vec![serde_json::from_slice(&e.into_payload().json).unwrap()],
                    Vec::new(),
                );
            }
        };
        let calls = self.harness().calls.clone();
        let output = block_on(schema.execute(
            Self::roots(),
            self,
            Operation {
                document,
                operation_name: operation_name.map(str::to_owned),
                variables,
            },
            options,
        ));
        let payloads = output
            .payloads
            .iter()
            .map(|p| serde_json::from_slice(&p.json).unwrap())
            .collect();
        let calls = calls.lock().unwrap().clone();
        (payloads, calls)
    }

    /// Drives the execution with a no-op waker until it stalls or finishes,
    /// and returns the payloads shipped so far: for queries that never complete.
    fn run_until_stalled(self, query: &str, options: ExecuteOptions) -> Vec<Value> {
        use futures::Stream;
        use std::task::{Context, Poll};
        let schema = Self::schema(None);
        let document = schema.parse(query).unwrap();
        let mut payloads = Vec::new();
        let stream = schema.execute_stream(
            Self::roots(),
            self,
            Operation {
                document,
                operation_name: None,
                variables: Value::Null,
            },
            options,
            |payload| serde_json::to_value(&payload).unwrap(),
        );
        let mut stream = std::pin::pin!(stream);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        for _ in 0..1000 {
            match stream.as_mut().poll_next(&mut cx) {
                Poll::Ready(Some(payload)) => payloads.push(payload),
                Poll::Ready(None) => break,
                Poll::Pending => {}
            }
        }
        payloads
    }

    /// The BFS single (non-incremental) response. Unless the world gates or
    /// panics, or the mode is Halt, the reference executor runs the same
    /// request and must agree on `data` and the error multiset.
    fn single(self, query: &str, variables: Value, options: ExecuteOptions) -> (Value, Calls) {
        self.single_operation(None, query, variables, options)
    }

    /// [`Area::single`] of the named operation of a multi-operation document.
    fn single_operation(
        self,
        operation_name: Option<&str>,
        query: &str,
        variables: Value,
        options: ExecuteOptions,
    ) -> (Value, Calls) {
        let oracle = (!self.harness().is_interactive()
            && options.error_behavior != greem::ErrorBehavior::Halt)
            .then(|| self.clone());
        let (response, calls) =
            self.single_unchecked_operation(operation_name, query, variables.clone(), options);
        if let Some(world) = oracle {
            let (reference, _) =
                world.reference_operation(operation_name, query, variables, options);
            assert_equivalent(&response, &reference);
        }
        (response, calls)
    }

    /// [`Area::single`] without the reference executor: for behaviour only
    /// the set-based executor has, such as a cardinality failure.
    fn single_unchecked(
        self,
        query: &str,
        variables: Value,
        options: ExecuteOptions,
    ) -> (Value, Calls) {
        self.single_unchecked_operation(None, query, variables, options)
    }

    fn single_unchecked_operation(
        self,
        operation_name: Option<&str>,
        query: &str,
        variables: Value,
        options: ExecuteOptions,
    ) -> (Value, Calls) {
        let options = ExecuteOptions {
            incremental: IncrementalDelivery::Disabled,
            ..options
        };
        let (mut payloads, calls) =
            self.run_operation(operation_name, query, variables, options, None);
        (payloads.remove(0), calls)
    }

    /// The reference executor's response and call count.
    fn reference(self, query: &str, variables: Value, options: ExecuteOptions) -> (Value, u64) {
        self.reference_operation(None, query, variables, options)
    }

    fn reference_operation(
        self,
        operation_name: Option<&str>,
        query: &str,
        variables: Value,
        options: ExecuteOptions,
    ) -> (Value, u64) {
        let schema = Self::schema(None);
        let document = match schema.parse(query) {
            Ok(d) => d,
            Err(e) => return (serde_json::from_slice(&e.into_payload().json).unwrap(), 0),
        };
        let out = block_on(greem_reference::execute(
            &schema,
            Self::roots(),
            self,
            Operation {
                document,
                operation_name: operation_name.map(str::to_owned),
                variables,
            },
            options,
        ));
        (out.response, out.calls)
    }
}

pub fn sorted_errors(v: &Value) -> Vec<Value> {
    let mut errors = v
        .get("errors")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    errors.sort_by_key(|e| e.to_string());
    errors
}

/// `data` equal and the errors equal as multisets.
pub fn assert_equivalent(bfs: &Value, reference: &Value) {
    assert_eq!(
        bfs.get("data"),
        reference.get("data"),
        "data differs\nbfs: {bfs}\nref: {reference}"
    );
    assert_eq!(
        sorted_errors(bfs),
        sorted_errors(reference),
        "errors differ\nbfs: {bfs}\nref: {reference}"
    );
}

/// A set-based `Resolver` impl that logs and paces the call through the
/// world's harness before running `$body`, which yields one output per parent.
#[macro_export]
macro_rules! resolver {
    ($world:ty; $ty:ty, $marker:path, $name:literal, $out:ty, |$parents:ident, $args:ident, $ctx:ident| $body:expr) => {
        impl ::greem::Resolver<$marker, $world> for $ty {
            type Output<'obj>
                = $out
            where
                Self: 'obj;
            async fn resolve<'obj, 'call>(
                $parents: &'call [&'obj Self],
                $args: &'obj ::greem::Args<$marker>,
                $ctx: &'obj ::greem::Context<'obj, $world>,
            ) -> ::core::result::Result<Vec<Self::Output<'obj>>, ::greem::Error>
            where
                'obj: 'call,
            {
                $crate::harness::HasHarness::harness($ctx.app())
                    .trace($name, $parents.len())
                    .await;
                let out: Vec<Self::Output<'obj>> = $body;
                Ok(out)
            }
        }
    };
}
