//! The schema and application shared by the runtime, macro and reference
//! tests: `greem-build`'s output for `schema.graphql`, and hand-written
//! resolvers over it.
//!
//! Only integration tests (`tests/*.rs`) can use it: a `#[cfg(test)]` unit
//! test inside `greem` links a second copy of `greem` whose types differ.

pub mod schema {
    greem::include_schema!();
}

pub mod app;
