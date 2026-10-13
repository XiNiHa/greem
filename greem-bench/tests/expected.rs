//! Each workload at its smallest size still produces its checked-in
//! `expected.json`. `GREEM_BENCH_BLESS=1` rewrites the files instead.

use greem_bench::app::build_schema;
use greem_bench::run;
use greem_bench::workload::Workload;

#[test]
fn every_workload_matches_its_expected_output() {
    let schema = build_schema();
    let runtime = run::runtime();
    let bless = std::env::var_os("GREEM_BENCH_BLESS").is_some();
    let mut mismatches = Vec::new();
    for workload in Workload::all() {
        let payloads = runtime.block_on(run::payloads(&schema, &workload, workload.smallest()));
        let actual = if workload.incremental() {
            serde_json::Value::Array(payloads)
        } else {
            let [response] = <[_; 1]>::try_from(payloads).expect("one response");
            response
        };
        let path = workload.expected_path();
        if bless {
            std::fs::write(&path, serde_json::to_vec(&actual).unwrap()).unwrap();
            continue;
        }
        let expected: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        )
        .unwrap();
        if actual != expected {
            mismatches.push(workload.name.clone());
        }
    }
    assert!(
        mismatches.is_empty(),
        "mismatched workloads: {mismatches:?}"
    );
}
