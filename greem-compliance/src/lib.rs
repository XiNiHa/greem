//! The compliance crate: the harness every world embeds, the property schema
//! and its world, and one area schema per ported graphql-js suite. The BFS
//! executor is compared against `greem-reference` here.

pub mod harness;

pub mod schema {
    greem::include_schema!("property.rs");
}

pub mod world;

pub mod graphql_js;
