//! `servers.toml`: one entry per implementation, with the commands that build,
//! run and version it. Every command runs through `sh -c` in `cwd`, relative
//! to the manifest; `run` gets `PORT` and `WORKLOADS`.

use greem_bench::workload::Capability;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub implementation: Vec<Implementation>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Implementation {
    pub name: String,
    pub cwd: PathBuf,
    pub build: Option<String>,
    pub run: String,
    pub port: u16,
    pub version: String,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// How it is served, for the report: threads, process model.
    pub note: Option<String>,
}

impl Manifest {
    pub fn load(path: &Path) -> Manifest {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut manifest: Manifest =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let dir = path.parent().unwrap_or(Path::new("."));
        for implementation in &mut manifest.implementation {
            implementation.cwd = dir.join(&implementation.cwd);
        }
        manifest
    }
}

impl Implementation {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/graphql", self.port)
    }

    pub fn supports(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}
