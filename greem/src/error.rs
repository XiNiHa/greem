use serde::Serialize;
use std::fmt;

/// An execution error raised at one response position.
///
/// The executor fills in `path` and `locations`; resolvers supply only the
/// message and optional `extensions`.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    message: String,
    extensions: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            extensions: None,
        }
    }

    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions
            .get_or_insert_with(Default::default)
            .insert(key.into(), value.into());
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn extensions(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.extensions.as_ref()
    }

    pub(crate) fn framework(message: impl Into<String>, code: &'static str) -> Self {
        Self::new(message).with_extension("code", code)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self::new(error.to_string())
    }
}

/// A failure converting an input value into a Rust argument or input type.
#[derive(Clone, Debug, PartialEq)]
pub struct InputError {
    message: String,
    path: Vec<String>,
}

impl InputError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: Vec::new(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Prefixes the input path with the name of the field or argument being read.
    pub fn at(mut self, segment: &str) -> Self {
        self.path.insert(0, segment.to_owned());
        self
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path.join("."), self.message)
        }
    }
}

impl std::error::Error for InputError {}

/// A location in the request document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

/// One segment of an error path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

impl Serialize for PathSegment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PathSegment::Key(key) => serializer.serialize_str(key),
            PathSegment::Index(index) => serializer.serialize_u64(*index as u64),
        }
    }
}

/// An error as it appears in a response's `errors` list.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GraphQLError {
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<Location>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<PathSegment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Map<String, serde_json::Value>>,
}

impl GraphQLError {
    pub(crate) fn from_error(
        error: &Error,
        locations: Vec<Location>,
        path: Vec<PathSegment>,
    ) -> Self {
        Self {
            message: error.message.clone(),
            locations,
            path: Some(path),
            extensions: error.extensions.clone(),
        }
    }

    pub(crate) fn request(message: impl Into<String>, locations: Vec<Location>) -> Self {
        Self {
            message: message.into(),
            locations,
            path: None,
            extensions: None,
        }
    }
}

/// Errors produced while building a [`Schema`](crate::Schema).
#[derive(Debug)]
pub struct SchemaError(pub(crate) String);

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SchemaError {}
