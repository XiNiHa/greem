//! Each workload at its smallest size still produces its checked-in oracle:
//! `expected.json`, a merged response for an incremental workload.
//! `GREEM_BENCH_BLESS=1` rewrites the files instead.

use greem_bench::app::build_schema;
use greem_bench::merge::merge;
use greem_bench::run;
use greem_bench::workload::Workload;

#[test]
fn every_workload_matches_its_oracle() {
    let schema = build_schema();
    let runtime = run::runtime();
    let bless = std::env::var_os("GREEM_BENCH_BLESS").is_some();
    let mut mismatches = Vec::new();
    for workload in Workload::all() {
        let payloads = runtime.block_on(run::payloads(&schema, &workload, workload.smallest()));
        if workload.incremental() {
            assert!(
                payloads.len() > 1,
                "{}: delivered in one payload",
                workload.name
            );
        }
        let actual = merge(payloads);
        if bless {
            std::fs::write(workload.oracle_path(), serde_json::to_vec(&actual).unwrap()).unwrap();
            continue;
        }
        if actual != workload.oracle() {
            mismatches.push(workload.name.clone());
        }
    }
    assert!(
        mismatches.is_empty(),
        "mismatched workloads: {mismatches:?}"
    );
}

/// `deferred` and `streamed` select exactly what `wide` does, so their oracles
/// are `wide`'s: an independent check on the merge.
#[test]
fn incremental_oracles_equal_the_plain_selection() {
    let wide = Workload::load("wide").oracle();
    for name in ["deferred", "streamed"] {
        let workload = Workload::load(name);
        assert_eq!(workload.smallest(), 1000);
        assert_eq!(workload.oracle(), wide, "{name}");
    }
}
