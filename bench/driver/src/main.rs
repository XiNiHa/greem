//! The driver: builds and starts each implementation in `servers.toml` in
//! turn, checks it against every workload's oracle, times the cells it can
//! run, and writes the report.

mod http;
mod manifest;
mod multipart;
mod oracle;
mod process;
mod report;
mod run;
mod stats;

use clap::Parser;
use greem_bench::workload::Workload;
use manifest::Manifest;
use report::{Cell, Header, Outcome, Report, WorkloadNote};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Parser)]
struct Args {
    /// The implementations to measure, by manifest entry.
    #[arg(long = "manifest", default_value = "servers.toml")]
    manifest: PathBuf,
    /// Measure a manifest entry at this URL instead of starting it:
    /// `name=http://host:port/graphql`.
    #[arg(long = "server", value_parser = parse_server)]
    running: Vec<(String, String)>,
    /// Only these implementations.
    #[arg(long = "only")]
    only: Vec<String>,
    /// Only these workloads.
    #[arg(long = "workload")]
    workloads: Vec<String>,
    /// `all` sizes of each workload, or its `smallest` only.
    #[arg(long, default_value = "all")]
    sizes: Sizes,
    /// Concurrent connections per cell.
    #[arg(long, value_delimiter = ',', default_value = "1,32")]
    concurrency: Vec<usize>,
    /// Unrecorded requests per connection before each cell's window, so a
    /// JIT runtime is warm at every size.
    #[arg(long, default_value_t = 20)]
    warmup: usize,
    /// Seconds each cell is measured for.
    #[arg(long, default_value_t = 5.0)]
    duration: f64,
    /// Where the JSON result goes, with the Markdown report beside it as
    /// `.md`; `results/<started>.json` beside the manifest.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Sizes {
    All,
    Smallest,
}

fn parse_server(arg: &str) -> Result<(String, String), String> {
    arg.split_once('=')
        .map(|(name, url)| (name.to_owned(), url.to_owned()))
        .ok_or_else(|| "expected name=url".to_owned())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let manifest = Manifest::load(&args.manifest);
    let workloads: Vec<Workload> = Workload::all()
        .into_iter()
        .filter(|w| args.workloads.is_empty() || args.workloads.contains(&w.name))
        .collect();
    let implementations = manifest
        .implementation
        .iter()
        .filter(|i| args.only.is_empty() || args.only.contains(&i.name));
    let workloads_dir = Path::new(greem_bench::workload::DIR)
        .canonicalize()
        .expect("workloads directory");
    let duration = Duration::from_secs_f64(args.duration);
    let started = timestamp();
    let mut versions = Vec::new();
    let mut cells = Vec::new();
    for implementation in implementations {
        let elsewhere = args
            .running
            .iter()
            .find(|(name, _)| name == &implementation.name);
        let (url, running) = match elsewhere {
            Some((_, url)) => (url.clone(), None),
            None => {
                process::build(implementation).await;
                let running =
                    process::start(implementation, &workloads_dir, &http::connection()).await;
                (implementation.url(), Some(running))
            }
        };
        versions.push(report::Implementation {
            name: implementation.name.clone(),
            version: process::version(implementation).await,
            note: implementation.note.clone(),
        });
        for workload in &workloads {
            let sizes = match args.sizes {
                Sizes::All => &workload.sizes[..],
                Sizes::Smallest => &workload.sizes[..1],
            };
            let missing = workload
                .requires
                .iter()
                .find(|&&c| !implementation.supports(c));
            let blocked = match missing {
                Some(capability) => Some(Outcome::Skipped {
                    reason: format!("requires {capability}"),
                }),
                None => oracle::check(&url, workload)
                    .await
                    .err()
                    .map(|reason| Outcome::Failed { reason }),
            };
            for &size in sizes {
                for &concurrency in &args.concurrency {
                    let outcome = match &blocked {
                        Some(outcome) => outcome.clone(),
                        None => {
                            let request = Arc::new(http::Request::new(&url, workload, size));
                            match run::measure(request, concurrency, args.warmup, duration).await {
                                Ok(measurement) => Outcome::Measured(measurement),
                                Err(reason) => Outcome::Failed { reason },
                            }
                        }
                    };
                    eprintln!(
                        "[{}] {} size {size} concurrency {concurrency}: {}",
                        implementation.name,
                        workload.name,
                        outcome.summary()
                    );
                    cells.push(Cell {
                        workload: workload.name.clone(),
                        size,
                        implementation: implementation.name.clone(),
                        concurrency,
                        outcome,
                    });
                }
            }
        }
        if let Some(running) = running {
            running.stop().await;
        }
    }
    let report = Report {
        header: Header {
            started: started.clone(),
            os: os(),
            cpu: cpu(),
            cores: std::thread::available_parallelism().map_or(0, |n| n.get()),
            warmup_requests: args.warmup,
            duration_secs: args.duration,
            implementations: versions,
            workloads: workloads
                .iter()
                .map(|w| WorkloadNote {
                    name: w.name.clone(),
                    incremental: w.incremental(),
                    note: w.note.clone(),
                })
                .collect(),
        },
        cells,
    };
    let out = args.out.unwrap_or_else(|| {
        args.manifest
            .parent()
            .unwrap_or(Path::new("."))
            .join("results")
            .join(format!("{}.json", started.replace(':', "-")))
    });
    std::fs::create_dir_all(out.parent().unwrap_or(Path::new("."))).expect("results directory");
    std::fs::write(&out, serde_json::to_vec_pretty(&report).unwrap()).expect("write results");
    let markdown = report::markdown(&report);
    let out_md = out.with_extension("md");
    std::fs::write(&out_md, &markdown).expect("write report");
    print!("{markdown}");
    eprintln!("wrote {} and {}", out.display(), out_md.display());
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn os() -> String {
    command("uname", &["-sr"]).unwrap_or_else(|| std::env::consts::OS.to_owned())
}

fn cpu() -> String {
    if let Some(cpu) = command("sysctl", &["-n", "machdep.cpu.brand_string"]) {
        return cpu;
    }
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, name)| name.trim().to_owned())
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned())
}

/// Now in UTC as `YYYY-MM-DDTHH:MM:SSZ`.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil-from-days, Howard Hinnant's algorithm.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
