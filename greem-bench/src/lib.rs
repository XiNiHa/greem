//! The bench crate: portable workloads under `workloads/`, the greem
//! implementation of their resolver contract, and the pieces every consumer
//! shares to run one.

pub mod schema {
    greem::include_schema!();
}

pub mod app;
pub mod run;
pub mod workload;
