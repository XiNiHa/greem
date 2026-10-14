//! What a run produces: the JSON result file and the Markdown tables, one per
//! concurrency level, with each other implementation's p50 as a multiple of
//! greem's, under the notes a reader needs to compare the cells.

use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::Write;

pub const GREEM: &str = "greem";

#[derive(Serialize)]
pub struct Report {
    pub header: Header,
    pub cells: Vec<Cell>,
}

#[derive(Serialize)]
pub struct Header {
    pub started: String,
    pub os: String,
    pub cpu: String,
    pub cores: usize,
    pub warmup_requests: usize,
    pub duration_secs: f64,
    pub implementations: Vec<Implementation>,
    pub workloads: Vec<WorkloadNote>,
}

#[derive(Serialize)]
pub struct Implementation {
    pub name: String,
    pub version: String,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct WorkloadNote {
    pub name: String,
    pub incremental: bool,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct Cell {
    pub workload: String,
    pub size: u32,
    pub implementation: String,
    pub concurrency: usize,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Serialize, Clone)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Measured(Measurement),
    Skipped { reason: String },
    Failed { reason: String },
}

#[derive(Serialize, Clone)]
pub struct Measurement {
    pub samples: usize,
    /// Time to the last byte.
    pub p50_ms: f64,
    /// Only with enough samples for a p99 to mean anything.
    pub p99_ms: Option<f64>,
    pub rps: f64,
    /// Time to the first complete payload, for incremental workloads.
    pub first_payload_p50_ms: Option<f64>,
    pub first_payload_p99_ms: Option<f64>,
    /// Median number of payloads a response arrived in, for incremental
    /// workloads.
    pub payloads: Option<usize>,
}

impl Outcome {
    /// One line for a progress message or a table cell.
    pub fn summary(&self) -> String {
        match self {
            Outcome::Skipped { reason } => format!("skipped: {reason}"),
            Outcome::Failed { reason } => format!("failed: {reason}"),
            Outcome::Measured(m) => {
                let mut text = format!("p50 {:.2} ms", m.p50_ms);
                if let Some(p99) = m.p99_ms {
                    write!(text, ", p99 {p99:.2} ms").unwrap();
                }
                if let Some(first) = m.first_payload_p50_ms {
                    write!(text, ", first p50 {first:.2} ms").unwrap();
                    if let Some(p99) = m.first_payload_p99_ms {
                        write!(text, ", p99 {p99:.2} ms").unwrap();
                    }
                }
                if let Some(payloads) = m.payloads {
                    write!(text, ", {payloads} payloads").unwrap();
                }
                write!(text, ", {:.0}/s, n={}", m.rps, m.samples).unwrap();
                text
            }
        }
    }
}

pub fn markdown(report: &Report) -> String {
    let h = &report.header;
    let mut out = format!(
        "# Bench {}\n\n{}, {}, {} cores. {} warm-up requests per connection, then {} s measured per cell.\n\n",
        h.started, h.os, h.cpu, h.cores, h.warmup_requests, h.duration_secs
    );
    for implementation in &h.implementations {
        write!(out, "- {} {}", implementation.name, implementation.version).unwrap();
        if let Some(note) = &implementation.note {
            write!(out, ": {note}").unwrap();
        }
        out.push('\n');
    }
    out.push_str(
        "\n## Reading the cells\n\n\
         Each cell is a closed loop: every connection sends its next request as soon as the last response ends, so at concurrency 1 the p50 is one request's latency and the ratio to greem is a throughput ratio. Above concurrency 1 an implementation on one thread competes with one on a worker per core, so those tables measure cores as much as executors. \
         `p99` appears only with at least 100 samples. For an incremental workload `first` is the time to the first complete multipart payload and `payloads` the median number a response arrived in; implementations batch differently, so the final byte is the comparable moment.\n",
    );
    let notes: Vec<&WorkloadNote> = h.workloads.iter().filter(|w| w.note.is_some()).collect();
    if !notes.is_empty() {
        out.push('\n');
        for workload in notes {
            writeln!(
                out,
                "- `{}`: {}",
                workload.name,
                workload.note.as_deref().unwrap_or_default()
            )
            .unwrap();
        }
    }
    let levels: BTreeSet<usize> = report.cells.iter().map(|c| c.concurrency).collect();
    for level in levels {
        write!(out, "\n## Concurrency {level}\n\n| Workload | Size |").unwrap();
        for implementation in &h.implementations {
            write!(out, " {} |", implementation.name).unwrap();
        }
        write!(out, "\n| --- | ---: |").unwrap();
        for _ in &h.implementations {
            write!(out, " --- |").unwrap();
        }
        out.push('\n');
        let rows: BTreeSet<(&str, u32)> = report
            .cells
            .iter()
            .filter(|c| c.concurrency == level)
            .map(|c| (c.workload.as_str(), c.size))
            .collect();
        for (workload, size) in rows {
            let cell = |implementation: &str| {
                report.cells.iter().find(|c| {
                    c.concurrency == level
                        && c.workload == workload
                        && c.size == size
                        && c.implementation == implementation
                })
            };
            let baseline = cell(GREEM).and_then(|c| match &c.outcome {
                Outcome::Measured(m) => Some(m.p50_ms),
                _ => None,
            });
            write!(out, "| {workload} | {size} |").unwrap();
            for implementation in &h.implementations {
                let mut text = String::new();
                if let Some(cell) = cell(&implementation.name) {
                    text = cell.outcome.summary();
                    if let (Outcome::Measured(m), Some(baseline)) = (&cell.outcome, baseline)
                        && implementation.name != GREEM
                    {
                        write!(text, ", {:.2}x greem", m.p50_ms / baseline).unwrap();
                    }
                }
                write!(out, " {text} |").unwrap();
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(
        workload: &str,
        implementation: &str,
        concurrency: usize,
        measurement: Measurement,
    ) -> Cell {
        Cell {
            workload: workload.into(),
            size: 1000,
            implementation: implementation.into(),
            concurrency,
            outcome: Outcome::Measured(measurement),
        }
    }

    fn plain(samples: usize, p50_ms: f64, p99_ms: Option<f64>, rps: f64) -> Measurement {
        Measurement {
            samples,
            p50_ms,
            p99_ms,
            rps,
            first_payload_p50_ms: None,
            first_payload_p99_ms: None,
            payloads: None,
        }
    }

    fn report() -> Report {
        Report {
            header: Header {
                started: "2026-10-14T10:00:00Z".into(),
                os: "macos".into(),
                cpu: "Apple M3".into(),
                cores: 8,
                warmup_requests: 20,
                duration_secs: 5.0,
                implementations: vec![
                    Implementation {
                        name: "greem".into(),
                        version: "fb86da1".into(),
                        note: Some("a tokio worker per core".into()),
                    },
                    Implementation {
                        name: "graphql-js".into(),
                        version: "17.0.2".into(),
                        note: None,
                    },
                ],
                workloads: vec![
                    WorkloadNote {
                        name: "deferred".into(),
                        incremental: true,
                        note: None,
                    },
                    WorkloadNote {
                        name: "wide".into(),
                        incremental: false,
                        note: Some("the plain selection".into()),
                    },
                ],
            },
            cells: vec![
                measured("wide", "greem", 1, plain(11900, 0.42, Some(0.8), 2380.0)),
                measured(
                    "wide",
                    "graphql-js",
                    1,
                    plain(7000, 0.7, Some(1.25), 1400.0),
                ),
                measured(
                    "deferred",
                    "greem",
                    1,
                    Measurement {
                        samples: 50,
                        p50_ms: 1.0,
                        p99_ms: None,
                        rps: 10.0,
                        first_payload_p50_ms: Some(0.3),
                        first_payload_p99_ms: None,
                        payloads: Some(4),
                    },
                ),
                Cell {
                    workload: "deferred".into(),
                    size: 1000,
                    implementation: "graphql-js".into(),
                    concurrency: 1,
                    outcome: Outcome::Failed {
                        reason: "oracle mismatch".into(),
                    },
                },
                measured("wide", "greem", 32, plain(60000, 10.0, Some(20.0), 12000.0)),
                Cell {
                    workload: "wide".into(),
                    size: 1000,
                    implementation: "graphql-js".into(),
                    concurrency: 32,
                    outcome: Outcome::Skipped {
                        reason: "requires stream".into(),
                    },
                },
            ],
        }
    }

    #[test]
    fn one_table_per_concurrency_level_with_ratios_to_greem() {
        let text = markdown(&report());
        let (head, tables) = text.split_once("\n## Concurrency 1\n").unwrap();
        assert!(head.starts_with(
            "# Bench 2026-10-14T10:00:00Z\n\n\
             macos, Apple M3, 8 cores. 20 warm-up requests per connection, then 5 s measured per cell.\n\n\
             - greem fb86da1: a tokio worker per core\n\
             - graphql-js 17.0.2\n\n\
             ## Reading the cells\n"
        ), "{head}");
        assert!(head.ends_with("- `wide`: the plain selection\n"), "{head}");
        assert_eq!(
            tables,
            "\n| Workload | Size | greem | graphql-js |\n\
             | --- | ---: | --- | --- |\n\
             | deferred | 1000 | p50 1.00 ms, first p50 0.30 ms, 4 payloads, 10/s, n=50 | failed: oracle mismatch |\n\
             | wide | 1000 | p50 0.42 ms, p99 0.80 ms, 2380/s, n=11900 | p50 0.70 ms, p99 1.25 ms, 1400/s, n=7000, 1.67x greem |\n\
             \n## Concurrency 32\n\n\
             | Workload | Size | greem | graphql-js |\n\
             | --- | ---: | --- | --- |\n\
             | wide | 1000 | p50 10.00 ms, p99 20.00 ms, 12000/s, n=60000 | skipped: requires stream |\n"
        );
    }

    #[test]
    fn the_json_names_each_outcome() {
        let json = serde_json::to_value(report()).unwrap();
        assert_eq!(json["cells"][0]["outcome"], "measured");
        assert_eq!(json["cells"][0]["p50_ms"], 0.42);
        assert_eq!(json["cells"][2]["p99_ms"], serde_json::Value::Null);
        assert_eq!(json["cells"][2]["payloads"], 4);
        assert_eq!(json["cells"][3]["outcome"], "failed");
        assert_eq!(json["cells"][3]["reason"], "oracle mismatch");
        assert_eq!(json["header"]["implementations"][1]["version"], "17.0.2");
        assert_eq!(json["header"]["workloads"][0]["incremental"], true);
    }
}
