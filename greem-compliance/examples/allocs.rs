//! Allocations and time per request for the BFS executor over the compliance
//! world. A stand-in until greem-bench exists; delete it then.
//!
//! cargo run --release -p greem-compliance --example allocs [workload...]
//!
//! `USERS=n` sizes the user-list workloads (default 10000).

use futures::StreamExt;
use futures::executor::block_on;
use greem::{ExecuteOptions, IncrementalDelivery, Operation, Roots};
use greem_compliance::world::{MutationRoot, QueryRoot, World, build_schema};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

const WARMUP: usize = 3;
const ITERATIONS: usize = 30;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

fn grew(added: usize, removed: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(added, Relaxed);
    let live = LIVE.fetch_add(added, Relaxed) + added;
    LIVE.fetch_sub(removed, Relaxed);
    PEAK.fetch_max(live, Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grew(layout.size(), 0);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grew(layout.size(), 0);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        grew(new_size, layout.size());
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct Workload {
    name: &'static str,
    users: u32,
    posts_per_user: u32,
    query: String,
    incremental: bool,
}

fn workloads() -> Vec<Workload> {
    let n: u32 = std::env::var("USERS")
        .ok()
        .map(|v| v.parse().expect("USERS is a number"))
        .unwrap_or(10_000);
    let deep = format!(
        "{{ users {{ id {}{}}} }}",
        "friends { id ".repeat(12),
        "} ".repeat(12)
    );
    vec![
        Workload {
            name: "wide",
            users: n,
            posts_per_user: 3,
            query: format!("{{ users(first: {n}) {{ id name email posts {{ id title tags }} }} }}"),
            incremental: false,
        },
        Workload {
            name: "deep",
            users: 3,
            posts_per_user: 0,
            query: deep,
            incremental: false,
        },
        Workload {
            name: "matrix",
            users: 300,
            posts_per_user: 0,
            query: "{ matrix }".into(),
            incremental: false,
        },
        Workload {
            name: "deferred",
            users: n,
            posts_per_user: 3,
            query: format!(
                "{{ users(first: {n}) {{ id ... @defer {{ name email posts {{ id title tags }} }} }} }}"
            ),
            incremental: true,
        },
        Workload {
            name: "streamed",
            users: n,
            posts_per_user: 3,
            query: format!(
                "{{ users(first: {n}) @stream {{ id name email posts {{ id title tags }} }} }}"
            ),
            incremental: true,
        },
        Workload {
            name: "streams",
            users: n,
            posts_per_user: 3,
            query: format!(
                "{{ users(first: {n}) {{ id drafts @stream(initialCount: 1) {{ id title }} }} }}"
            ),
            incremental: true,
        },
    ]
}

struct Run {
    allocs: usize,
    bytes: usize,
    peak: usize,
    time: Duration,
    payloads: usize,
    hash: u64,
}

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(hash, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn run(schema: &greem_compliance::world::Schema, w: &Workload, op: Operation) -> Run {
    let world = World::seeded(w.users, w.posts_per_user);
    let options = ExecuteOptions {
        incremental: if w.incremental {
            IncrementalDelivery::Enabled
        } else {
            IncrementalDelivery::Disabled
        },
        ..Default::default()
    };
    let roots = Roots {
        query: QueryRoot,
        mutation: MutationRoot,
    };
    let (allocs, bytes, live) = (
        ALLOCS.load(Relaxed),
        BYTES.load(Relaxed),
        LIVE.load(Relaxed),
    );
    PEAK.store(live, Relaxed);
    let start = Instant::now();
    let mut buf = Vec::new();
    let (payloads, hash) = block_on(
        schema
            .execute_stream(roots, world, op, options, move |payload| {
                buf.clear();
                serde_json::to_writer(&mut buf, &payload).expect("serialize payload");
                fnv(0xcbf29ce484222325, &buf)
            })
            .fold((0, 0xcbf29ce484222325), |(n, acc), h| async move {
                (n + 1, fnv(acc, &h.to_le_bytes()))
            }),
    );
    Run {
        time: start.elapsed(),
        allocs: ALLOCS.load(Relaxed) - allocs,
        bytes: BYTES.load(Relaxed) - bytes,
        peak: PEAK.load(Relaxed) - live,
        payloads,
        hash,
    }
}

fn main() {
    let filter: Vec<String> = std::env::args().skip(1).collect();
    let schema = build_schema();
    println!(
        "{:<10} {:>12} {:>14} {:>14} {:>10} {:>9} {:>18}",
        "workload", "allocs", "bytes", "peak bytes", "median ms", "payloads", "output hash"
    );
    for w in workloads() {
        if !filter.is_empty() && !filter.iter().any(|f| f == w.name) {
            continue;
        }
        let document = schema.parse(&w.query).expect("workload parses");
        let op = || Operation {
            document: document.clone(),
            operation_name: None,
            variables: serde_json::Value::Null,
        };
        for _ in 0..WARMUP {
            run(&schema, &w, op());
        }
        let runs: Vec<Run> = (0..ITERATIONS).map(|_| run(&schema, &w, op())).collect();
        let mut times: Vec<Duration> = runs.iter().map(|r| r.time).collect();
        times.sort();
        let last = runs.last().unwrap();
        println!(
            "{:<10} {:>12} {:>14} {:>14} {:>10.2} {:>9} {:>18x}",
            w.name,
            last.allocs,
            last.bytes,
            last.peak,
            times[ITERATIONS / 2].as_secs_f64() * 1e3,
            last.payloads,
            last.hash
        );
    }
}
