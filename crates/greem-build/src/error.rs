use std::fmt::Debug;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GreemBuildError {
    #[error("Invalid codegen config")]
    InvalidConfig,
    #[error("Multiple definition found")]
    MultipleDefinitionFound(CollidedDefinition),
    #[error("Name collision between different kind of definitions found")]
    NameCollision(String, NameCollisionReason),
}

#[derive(Debug)]
pub enum CollidedDefinition {
    Schema,
    Scalar(String),
    ObjectField(String),
}

#[derive(Debug)]
pub enum NameCollisionReason {
    DifferentType,
    DifferentContent,
}
