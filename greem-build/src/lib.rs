//! Schema compilation: turns SDL into the generated schema module from a
//! build script, Tonic-style.
//!
//! ```no_run
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     greem_build::compile("schema.graphql")?;
//!     Ok(())
//! }
//! ```

mod codegen;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// A ready-made [`Scalar`](https://docs.rs/greem) implementation for a
/// custom scalar declared in the SDL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// `uuid::Uuid` (requires the `uuid` crate).
    Uuid,
    /// `serde_json::Value` (requires the `serde_json` crate).
    Json,
    /// A `String` passthrough.
    String,
    /// An `i64` passthrough (input accepts Int literals and numeric strings).
    I64,
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Schema compilation settings; start with [`configure`].
pub struct Config {
    out_dir: Option<PathBuf>,
    file_name: String,
    emit_rerun_if_changed: bool,
    scalars: BTreeMap<String, Codec>,
    absent_aware: Vec<String>,
}

pub fn configure() -> Config {
    Config {
        out_dir: None,
        file_name: "greem.rs".to_owned(),
        emit_rerun_if_changed: true,
        scalars: BTreeMap::new(),
        absent_aware: Vec::new(),
    }
}

/// Compiles one SDL file with the default settings into `OUT_DIR/greem.rs`.
pub fn compile(path: impl AsRef<Path>) -> Result<(), Error> {
    configure().compile(&[path.as_ref()])
}

impl Config {
    /// Where the generated file goes (default: `OUT_DIR`).
    pub fn out_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.out_dir = Some(dir.as_ref().to_owned());
        self
    }

    /// The generated file's name (default `greem.rs`); include it with
    /// `greem::include_schema!("name.rs")`.
    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = name.into();
        self
    }

    /// Whether to print `cargo:rerun-if-changed` for every SDL file (default true).
    pub fn emit_rerun_if_changed(mut self, emit: bool) -> Self {
        self.emit_rerun_if_changed = emit;
        self
    }

    /// Serves a custom scalar with a ready-made codec instead of a
    /// hand-written `impl greem::Scalar for types::Name`.
    pub fn scalar(mut self, name: impl Into<String>, codec: Codec) -> Self {
        self.scalars.insert(name.into(), codec);
        self
    }

    /// Input object types whose nullable, default-less fields use
    /// `greem::Maybe<T>` to distinguish absent from `null`.
    pub fn absent_aware(mut self, types: &[&str]) -> Self {
        self.absent_aware
            .extend(types.iter().map(|t| (*t).to_owned()));
        self
    }

    /// Merges the SDL files into one schema and writes the generated module.
    pub fn compile(self, paths: &[impl AsRef<Path>]) -> Result<(), Error> {
        let mut sources = Vec::new();
        for path in paths {
            let path = path.as_ref();
            let text = std::fs::read_to_string(path)
                .map_err(|e| Error(format!("cannot read {}: {e}", path.display())))?;
            if self.emit_rerun_if_changed {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            sources.push((path.display().to_string(), text));
        }
        if self.emit_rerun_if_changed {
            println!("cargo:rerun-if-changed=build.rs");
        }
        let code = self.generate(&sources)?;
        let out_dir = match &self.out_dir {
            Some(dir) => dir.clone(),
            None => PathBuf::from(std::env::var("OUT_DIR").map_err(|_| {
                Error("OUT_DIR is not set; call from build.rs or set out_dir".into())
            })?),
        };
        let target = out_dir.join(&self.file_name);
        std::fs::write(&target, code)
            .map_err(|e| Error(format!("cannot write {}: {e}", target.display())))?;
        Ok(())
    }

    /// Generates the module source for in-memory SDL (used by tests).
    pub fn generate(&self, sources: &[(String, String)]) -> Result<String, Error> {
        codegen::generate(sources, &self.scalars, &self.absent_aware)
    }
}

/// The `@defer` definition greem supplies when the SDL does not declare it.
pub const DEFER_DIRECTIVE: &str =
    "directive @defer(if: Boolean! = true, label: String) on FRAGMENT_SPREAD | INLINE_FRAGMENT\n";

/// The `@stream` definition greem supplies when the SDL does not declare it.
pub const STREAM_DIRECTIVE: &str =
    "directive @stream(if: Boolean! = true, label: String, initialCount: Int = 0) on FIELD\n";
