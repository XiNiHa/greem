//! The bench crate: portable workloads under `bench/workloads/`, the greem
//! implementation of their resolver contract, and the pieces every consumer
//! shares to run one.

pub mod schema {
    greem::include_schema!();
}

pub mod app;
pub mod merge;
pub mod run;
#[cfg(feature = "http")]
pub mod serve;
pub mod workload;
