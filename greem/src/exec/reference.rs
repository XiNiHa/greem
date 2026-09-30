//! Support for the depth-first reference executor: a per-position completion
//! context that builds `serde_json::Value` directly and bubbles nulls the
//! spec's way. The walk itself lives in the `greem-reference` crate.

use crate::context::{Context, HintAddr};
use crate::error::{Error, GraphQLError, Location, PathSegment};
use crate::exec::state::ErrorBehavior;
use crate::plan::{Leaf as LeafPath, PlanHeader, PlanId, PlanTable};
use crate::resolver::{Args, Field, Outputs, Resolver, Shape};
use crate::tree::{FieldKind, NodeId};
use futures::future::BoxFuture;
use std::sync::{Arc, Mutex};

/// `Err(())` means the position errored and must propagate if non-null.
pub type RefValue = Result<serde_json::Value, ()>;

/// Shared per-request state of a reference execution.
pub struct RefShared<C> {
    pub table: Arc<PlanTable>,
    pub app: Arc<C>,
    pub behavior: ErrorBehavior,
    pub introspection: Option<serde_json::Value>,
    pub errors: Mutex<Vec<GraphQLError>>,
    pub calls: Mutex<u64>,
    /// Mutation roots: stop after a root field nulls `data` (ticket 09).
    pub serial_root: bool,
}

/// The reference completion context for one position. Cheap to clone; every
/// nested completion works on its own copy.
pub struct RefCompletion<'s, C> {
    pub shared: &'s RefShared<C>,
    pub node: NodeId,
    pub leaf: LeafPath,
    pub path: Vec<PathSegment>,
    pub shape: Shape,
    pub level: usize,
    pub spans: Vec<Location>,
    pub next_index: usize,
}

impl<C> Clone for RefCompletion<'_, C> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared,
            node: self.node,
            leaf: self.leaf.clone(),
            path: self.path.clone(),
            shape: self.shape,
            level: self.level,
            spans: self.spans.clone(),
            next_index: self.next_index,
        }
    }
}

impl<'s, C> RefCompletion<'s, C> {
    pub fn new(
        shared: &'s RefShared<C>,
        node: NodeId,
        path: Vec<PathSegment>,
        shape: Shape,
        spans: Vec<Location>,
    ) -> Self {
        Self {
            shared,
            node,
            leaf: LeafPath::default(),
            path,
            shape,
            level: 0,
            spans,
            next_index: 0,
        }
    }

    pub fn item_nullable(&self) -> bool {
        self.shape.nullable_at(self.level + 1)
    }

    /// The context one list level down, at the next item index.
    pub fn item(&mut self) -> RefCompletion<'s, C> {
        let index = self.next_index;
        self.next_index += 1;
        let mut child = self.clone();
        child.path.push(PathSegment::Index(index));
        child.level += 1;
        child.next_index = 0;
        child
    }

    pub fn with_leaf(&self, step: u8) -> RefCompletion<'s, C> {
        let mut child = self.clone();
        child.leaf = self.leaf.push(step);
        child
    }

    /// A completed leaf, or its error; null at a non-null position is one.
    pub fn leaf<'v>(&self, value: Result<crate::value::Value<'_>, Error>) -> BoxFuture<'v, RefValue>
    where
        's: 'v,
    {
        match value {
            Ok(crate::value::Value::Null) if !self.shape.nullable_at(self.level) => {
                self.error(crate::value::null_at_non_null())
            }
            Ok(value) => {
                let json = value.to_json();
                Box::pin(async move { Ok(json) })
            }
            Err(error) => self.error(error),
        }
    }

    pub fn record(&self, error: &Error) {
        let error = GraphQLError::from_error(error, self.spans.clone(), self.path.clone());
        self.shared.errors.lock().unwrap().push(error);
    }

    pub fn error<'v>(&self, error: Error) -> BoxFuture<'v, RefValue>
    where
        's: 'v,
    {
        self.record(&error);
        let null_mode = self.shared.behavior == ErrorBehavior::Null;
        Box::pin(async move {
            if null_mode {
                Ok(serde_json::Value::Null)
            } else {
                Err(())
            }
        })
    }

    pub fn entry(&self) -> PlanId {
        self.shared
            .table
            .lookup(self.node, &self.leaf)
            .expect("plan entry for reference completion")
    }

    pub fn header(&self, entry: PlanId) -> &'s PlanHeader {
        self.shared.table.header(entry)
    }

    pub fn typed<P: 'static>(&self, entry: PlanId) -> &'s P {
        self.shared.table.typed::<P>(entry)
    }

    pub fn context(&self, entry: PlanId, field: u32) -> Context<'s, C> {
        Context::new(
            &self.shared.app,
            Some(HintAddr {
                table: &self.shared.table,
                entry,
                field,
            }),
        )
    }

    pub fn introspection(&self, key: &str) -> serde_json::Value {
        self.shared
            .introspection
            .as_ref()
            .and_then(|v| v.get(key).cloned())
            .unwrap_or(serde_json::Value::Null)
    }

    /// The context for field `field` of `entry` below this object.
    pub fn child(&self, entry: PlanId, field: u32, shape: Shape) -> RefCompletion<'s, C> {
        let header = self.header(entry);
        let f = &header.fields[field as usize];
        let mut path = self.path.clone();
        path.push(PathSegment::Key(f.key.clone()));
        RefCompletion {
            shared: self.shared,
            node: f.child.unwrap_or(self.node),
            leaf: LeafPath::default(),
            path,
            shape,
            level: 0,
            spans: f.spans.clone(),
            next_index: 0,
        }
    }
}

#[allow(clippy::result_unit_err)]
pub fn nullable(inner: RefValue) -> RefValue {
    Ok(inner.unwrap_or(serde_json::Value::Null))
}

#[allow(clippy::result_unit_err)]
pub fn list(items: Vec<RefValue>, item_nullable: bool) -> RefValue {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Ok(value) => out.push(value),
            Err(()) if item_nullable => out.push(serde_json::Value::Null),
            Err(()) => return Err(()),
        }
    }
    Ok(serde_json::Value::Array(out))
}

/// Resolves one field of one object with a one-element parent slice and
/// completes its output depth-first, applying the field's own nullability.
pub fn reference_field<'o, 's: 'o, T, F, C>(
    value: &'o T,
    entry: PlanId,
    index: u32,
    args: &'s Result<Args<F>, Error>,
    rc: &RefCompletion<'s, C>,
) -> BoxFuture<'o, RefValue>
where
    T: Resolver<F, C>,
    F: Field,
    C: Send + Sync + 'o,
{
    let child = rc.child(entry, index, F::SHAPE);
    let ctx = rc.context(entry, index);
    Box::pin(async move {
        let ctx = ctx;
        let nullable = F::SHAPE.nullable_at(0);
        let result = match args {
            Err(error) => child.error(error.clone()).await,
            Ok(args) => {
                *child.shared.calls.lock().unwrap() += 1;
                match T::resolve(&[value], args, &ctx).await {
                    Err(error) => child.error(error).await,
                    Ok(mut outputs) if outputs.len() == 1 => {
                        let output = outputs.pop().expect("one output");
                        <T::Output<'_> as Outputs<F::Type, C>>::__reference(output, &child).await
                    }
                    Ok(outputs) => {
                        child
                            .error(Error::framework(
                                format!(
                                    "resolver for field `{}` returned {} outputs for 1 parent",
                                    F::NAME,
                                    outputs.len()
                                ),
                                "CARDINALITY",
                            ))
                            .await
                    }
                }
            }
        };
        match result {
            Err(()) if nullable => Ok(serde_json::Value::Null),
            other => other,
        }
    })
}

/// Serializes one object value: `__typename` and introspection inline, every
/// other field through `resolve`, which the generated code dispatches.
pub fn reference_object<'o, 's: 'o, T, C>(
    value: &'o T,
    rc: &RefCompletion<'s, C>,
    typename: &'static str,
    resolve: impl Fn(&'o T, PlanId, u32, &RefCompletion<'s, C>) -> BoxFuture<'o, RefValue> + Send + 'o,
) -> BoxFuture<'o, RefValue>
where
    T: Sync,
    C: Send + Sync + 'o,
{
    let entry = rc.entry();
    let header: &'s PlanHeader = rc.header(entry);
    let rc = rc.clone();
    let stop_early = rc.shared.serial_root && rc.path.is_empty();
    Box::pin(async move {
        let mut object = serde_json::Map::new();
        let mut failed = false;
        for (i, f) in header.fields.iter().enumerate() {
            let value = match f.kind {
                FieldKind::Typename => Ok(serde_json::Value::String(typename.to_owned())),
                FieldKind::Introspection => Ok(rc.introspection(&f.key)),
                FieldKind::Normal => resolve(value, entry, i as u32, &rc).await,
            };
            match value {
                Ok(value) => {
                    object.insert(f.key.clone(), value);
                }
                Err(()) => {
                    failed = true;
                    if stop_early {
                        break;
                    }
                }
            }
        }
        if failed {
            Err(())
        } else {
            Ok(serde_json::Value::Object(object))
        }
    })
}
