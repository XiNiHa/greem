//! Everything generated code and the reference executor touch. Public for
//! codegen, hidden from users, exempt from semver.

pub use crate::error::{GraphQLError, Location, PathSegment};
pub use crate::exec::complete::{
    Completes, Completion, FieldsCx, InnerKind, ObjectBatch, Pos, complete_as, complete_either,
    field, introspection_field, typename_field, walk_as, walk_either,
};
#[cfg(feature = "reference-executor")]
pub use crate::exec::reference::{
    RefCompletion, RefShared, RefValue, list, nullable, reference_field, reference_object,
};
pub use crate::exec::scope::FieldFuture;
pub use crate::exec::state::{GroupId, Shared};
pub use crate::plan::{FieldHeader, Leaf, PlanHeader, PlanId, PlanTable, Walker};
pub use crate::resolver::seal;
pub use crate::schema::SchemaInfo;
pub use crate::tree::{Abort, FieldKind, NodeId, Tree, UsageId};
pub use crate::value::{ToLeaf, null_at_non_null, read_field, read_field_with, scalar_from_input};
pub use futures;
pub use serde_json;

/// The largest stream turn list seen in this process: evidence that retired
/// turn slots are reused (memory follows in-flight work, not stream length).
pub static MAX_LIVE_TURNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The largest number of delivery groups alive at once in this process:
/// evidence that completed groups are reclaimed.
pub static MAX_LIVE_GROUPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The per-field argument payload stored in a typed Plan entry.
pub type FieldArgs<F> = Result<crate::resolver::Args<F>, crate::error::Error>;

/// Keeps its items when greem is built with the `reference-executor`
/// feature and drops them otherwise, so generated code can always emit its
/// reference completion.
#[cfg(feature = "reference-executor")]
#[doc(hidden)]
#[macro_export]
macro_rules! __reference_items {
    ($($item:tt)*) => { $($item)* };
}

#[cfg(not(feature = "reference-executor"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __reference_items {
    ($($item:tt)*) => {};
}

pub use crate::__reference_items as reference;
