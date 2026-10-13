//! The workload files: one directory each under `workloads/`, read as the
//! contract describes them.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/workloads");

#[derive(Debug, Clone)]
pub struct Workload {
    pub name: String,
    pub document: String,
    pub sizes: Vec<u32>,
    pub variables: serde_json::Map<String, serde_json::Value>,
    pub latency: BTreeMap<String, Duration>,
    pub requires: Vec<Capability>,
    dir: PathBuf,
}

/// What a framework must support to run a workload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Defer,
    Stream,
}

/// The shape of `workload.json`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    sizes: Vec<u32>,
    #[serde(default)]
    variables: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    latency_ms: BTreeMap<String, u64>,
    #[serde(default)]
    requires: Vec<Capability>,
}

impl Workload {
    /// Every workload under `workloads/`, by name.
    pub fn all() -> Vec<Workload> {
        let mut names: Vec<String> = std::fs::read_dir(DIR)
            .expect("workloads directory")
            .filter_map(|entry| {
                let entry = entry.expect("directory entry");
                entry
                    .file_type()
                    .expect("file type")
                    .is_dir()
                    .then(|| entry.file_name().to_string_lossy().into_owned())
            })
            .collect();
        names.sort();
        names.iter().map(|name| Workload::load(name)).collect()
    }

    pub fn load(name: &str) -> Workload {
        let dir = Path::new(DIR).join(name);
        let document =
            std::fs::read_to_string(dir.join("document.graphql")).expect("workload document");
        let manifest: Manifest = serde_json::from_str(
            &std::fs::read_to_string(dir.join("workload.json")).expect("workload file"),
        )
        .unwrap_or_else(|e| panic!("{name}/workload.json: {e}"));
        assert!(!manifest.sizes.is_empty(), "{name}: sizes is empty");
        Workload {
            name: name.to_owned(),
            document,
            sizes: manifest.sizes,
            variables: manifest.variables,
            latency: manifest
                .latency_ms
                .into_iter()
                .map(|(field, ms)| (field, Duration::from_millis(ms)))
                .collect(),
            requires: manifest.requires,
            dir,
        }
    }

    pub fn smallest(&self) -> u32 {
        self.sizes[0]
    }

    pub fn incremental(&self) -> bool {
        self.requires
            .iter()
            .any(|r| matches!(r, Capability::Defer | Capability::Stream))
    }

    /// The variables of one run: `$size` bound to `size` over `variables`.
    pub fn variables_at(&self, size: u32) -> serde_json::Value {
        let mut variables = self.variables.clone();
        variables.insert("size".into(), size.into());
        serde_json::Value::Object(variables)
    }

    pub fn expected_path(&self) -> PathBuf {
        self.dir.join("expected.json")
    }
}
