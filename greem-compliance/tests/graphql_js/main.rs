//! The graphql-js v17.0.2 execution test suites ported one-to-one onto the
//! area schemas in `greem_compliance::graphql_js`. Each module mirrors one
//! upstream file in its order; a case that cannot port is recorded in place
//! as `// it('<title>')` plus `// Not ported, reason (n): ...`, where n is
//!
//! - (i) per-object semantics: resolver call counts or order, `info`;
//! - (ii) a JS mechanism: iterator `return()`, promise timing as the subject,
//!   abort signals, `executeSync`;
//! - (iii) a graphql-js option greem lacks: `enableEarlyExecution`,
//!   `experimentalFragmentArguments`, `assumeValid`;
//! - (iv) vacuous, rejected at compile time: a null returned at a non-null
//!   position, a wrong runtime type, a non-list at a list position.

#[path = "../common/mod.rs"]
mod common;

mod r#abstract;
mod defer;
mod directives;
mod error_propagation;
mod executor;
mod lists;
mod mutations;
mod nonnull;
mod oneof;
mod schema;
mod stream;
mod subscribe;
mod union_interface;
mod variables;
