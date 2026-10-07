//! greem: a schema-first, breadth-first async GraphQL server framework.
//!
//! The generated schema module (see `greem-build`) supplies type tags and
//! field markers; applications implement [`Resolver`] per field and hand the
//! root values to [`Schema::execute`].

#![forbid(unsafe_code)]

mod context;
mod error;
mod exec;
pub mod http;
mod plan;
mod resolver;
mod schema;
mod tree;
mod value;

#[doc(hidden)]
pub mod __private;

pub use context::{Context, DeliveryGroup, HintRegistry, Planning};
pub use error::{Error, GraphQLError, InputError, Location, PathSegment, SchemaError};
pub use exec::payload::{Payload, PayloadKind};
pub use exec::state::{ErrorBehavior, ExecuteOptions, IncrementalDelivery};
pub use resolver::{
    Args, As, Either, Field, List, NoMutation, NoMutationType, Nullable, Outputs, Resolver, Shape,
    Streamed,
};
pub use schema::{
    ExecutionOutput, Operation, OwnedPayload, RequestErrors, Roots, Schema, SchemaBuilder,
};
pub use tree::Document;
pub use value::{Enum, FromInput, InputValue, Maybe, Scalar, Value, scalars};

#[cfg(feature = "macros")]
pub use greem_macros::object;

/// The greem-build version this runtime accepts generated code from.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Includes the generated schema module from `OUT_DIR`.
#[macro_export]
macro_rules! include_schema {
    () => {
        include!(concat!(env!("OUT_DIR"), "/greem.rs"));
    };
    ($file:literal) => {
        include!(concat!(env!("OUT_DIR"), "/", $file));
    };
}
