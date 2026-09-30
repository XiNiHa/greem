//! The compliance harness: the property schema, its world, and the resolvers
//! that are pure functions of the world. The BFS executor is compared against
//! `greem-reference` here.

pub mod schema {
    greem::include_schema!("property.rs");
}

pub mod world;
