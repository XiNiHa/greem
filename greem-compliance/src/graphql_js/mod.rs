//! The graphql-js v17.0.2 execution test suites, one area per upstream file:
//! `src/execution/__tests__/<name>-test.ts`, or `src/execution/incremental/__tests__/`
//! for `defer` and `stream`. Each area is a superset of every schema shape
//! its suite's cases build, with a literal world per case.
//!
//! Suites left out, as JS API surface or unit tests of internals with no
//! document-level claim: `cancellation` (an abort signal yields a response
//! carrying errors, while greem's cancellation is dropping the stream),
//! `sync`, `hooks`, `diagnostics-*`, `resolve`, `incremental`, the
//! `legacyIncremental` format, and the helper tests (`AsyncWorkTracker`,
//! `Queue`, `WorkQueue`, `Computation`, `collectFields`, `mapAsyncIterable`,
//! `simplePubSub`, `cancellablePromise`, `collectIteratorPromises`,
//! `withConcurrentAbruptClose`, `AbortedGraphQLExecutionError`).

pub mod r#abstract;
pub mod defer;
pub mod directives;
pub mod error_propagation;
pub mod executor;
pub mod lists;
pub mod mutations;
pub mod nonnull;
pub mod oneof;
pub mod schema;
pub mod stream;
pub mod subscribe;
pub mod union_interface;
pub mod variables;
