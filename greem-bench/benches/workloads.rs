//! Every workload under `bench/workloads/`. `GREEM_BENCH_SIZES=smallest` is the CI
//! shape: each workload's first size only, those declaring fetch latency
//! skipped.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use futures::StreamExt;
use greem_bench::app::build_schema;
use greem_bench::run;
use greem_bench::workload::Workload;
use std::hint::black_box;

fn workloads(c: &mut Criterion) {
    let smallest_only = std::env::var_os("GREEM_BENCH_SIZES").is_some_and(|v| v == "smallest");
    let runtime = run::runtime();
    let schema = build_schema();
    for workload in Workload::all() {
        if smallest_only && !workload.latency.is_empty() {
            continue;
        }
        let mut group = c.benchmark_group(&workload.name);
        group.bench_function("parse", |b| {
            b.iter(|| black_box(run::parse(&schema, &workload)))
        });
        let document = run::parse(&schema, &workload);
        let sizes = if smallest_only {
            &workload.sizes[..1]
        } else {
            &workload.sizes[..]
        };
        for &size in sizes {
            let op = || run::operation(&document, &workload, size);
            group.bench_with_input(BenchmarkId::new("execute", size), &size, |b, _| {
                b.to_async(&runtime).iter(|| {
                    run::execute(&schema, &workload, op(), |payload| {
                        black_box(&payload);
                    })
                    .for_each(|()| async {})
                })
            });
            group.bench_with_input(BenchmarkId::new("serialize", size), &size, |b, _| {
                b.to_async(&runtime).iter(|| {
                    run::execute(&schema, &workload, op(), run::encode_json()).for_each(
                        |len| async move {
                            black_box(len);
                        },
                    )
                })
            });
            group.bench_with_input(BenchmarkId::new("request", size), &size, |b, _| {
                b.to_async(&runtime).iter(|| {
                    run::execute_request(&schema, &workload, size, run::encode_json()).for_each(
                        |len| async move {
                            black_box(len);
                        },
                    )
                })
            });
        }
        group.finish();
    }
}

criterion_group!(benches, workloads);
criterion_main!(benches);
