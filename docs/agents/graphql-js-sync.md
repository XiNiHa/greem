# Syncing the graphql-js port

The suites under `greem-compliance/tests/graphql_js` mirror graphql-js's execution test files one-to-one at the tag pinned by `package.metadata.graphql_js` in `greem-compliance/Cargo.toml`. This brief is for the agent that moves the port to a newer tag, run by `.github/workflows/graphql-js-sync.yml` or by hand. The workflow names the target tag, the sync branch, the base branch, and two context files: `diff.md`, the upstream changes between the pin and the target tag, and `audit.txt`, the port's case paths diffed against the target tag.

`greem-compliance/scripts/graphql-js-sync.sh` produces the same context by hand: `diff TAG` and `audit TAG`.

## Read first

1. `AGENTS.md` and the Compliance section of `CONTEXT.md`.
2. The module doc of `greem-compliance/tests/graphql_js/main.rs`: the four reasons a case is recorded instead of ported.
3. The module doc of `greem-compliance/src/graphql_js/mod.rs`: which upstream files are excluded and why.
4. `greem-compliance/src/harness.rs` and `greem-compliance/tests/common/mod.rs`: the run helpers and the payload assertions.
5. The "Null propagation and error behavior" and "Incremental delivery" sections of `docs/spec.md`: the rules every expectation is derived from.
6. One touched suite in full: its `schemas/graphql_js/<suite>.graphql`, `src/graphql_js/<suite>.rs` and `tests/graphql_js/<suite>.rs`.

## The job

1. Set the pin to the target tag.
2. For every suite whose upstream file changed: port each added case at its upstream position, re-derive each changed case, and remove each case upstream removed, so the audit at the target tag passes. Extend the suite's area schema only when a changed case needs a shape it lacks.
3. For every upstream file added under the watched directories: decide by the criteria in `mod.rs` whether it makes document-level claims. If it does, add it as a new area, following `build.rs`, `src/lib.rs`, `mod.rs` and `main.rs`, with a header `//! graphql-js \`<upstream path>\`` on the first line of its test file. If not, add it to the exclusion list in `mod.rs` with its reason.
4. For every upstream file removed: remove nothing, report it.
5. Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test -p greem-compliance --test graphql_js` and `graphql-js-sync.sh audit <tag>`.
6. Commit everything as one commit, `sync graphql-js <tag>`, with the `Co-Authored-By` trailer Claude Code gives the running model, push the branch, and open a draft pull request with the same title against the base branch, labelled `ready-for-human`.

When `diff.md` shows no change to any watched file and no file added or removed, stop: say so in the run output and open nothing.

## Rules of a ported case

- Every upstream `it` is one `#[test]` with `/// it('<verbatim title>')` above it and a snake_case slug of the title as its name, or is recorded in place as `// it('<verbatim title>')` plus `// Not ported, reason (n): <why>`. Every upstream `describe` is a `mod` with `/// describe('<verbatim title>')` above it. Order and nesting follow upstream exactly; the audit compares the ordered `describe > ... > it` paths. A second test split from one case (its async half, say) carries a plain doc comment, never a second marker.
- Query text is verbatim, including its leading newline and indentation, so `locations` match. Variables go through `serde_json::json!`.
- Port by translation whenever the case still makes a document-level claim: a resolver that throws is `Err(Error::new(<upstream's message>))`, a rejected promise is the same after a yield, `resolveOnNextTick` is a yield, an async iterable that errors is a `Streamed` source yielding `Err`. Record a case only when the mechanism itself is what it asserts.
- Expectations are greem's full payload sequence, derived by hand from `docs/spec.md`, asserted whole with `assert_response` or `assert_payloads`. Never paste greem's output into an expectation. Where greem's batching legitimately differs from upstream's sequence, write greem's and leave upstream's in a comment with the reason. Framework error messages are greem's own; resolver error messages reuse upstream's text.
- `single()` cross-checks the reference executor; use it for non-incremental cases, `run()` or `run_at_capacity()` for incremental ones, `single_unchecked()` only for behaviour the set-based executor alone has.
- When greem contradicts the spec, keep the spec-correct expectation and mark the test `#[ignore = "divergence: <one line>"]`. When a case needs a feature greem lacks, write it in full and mark it `#[ignore = "#<issue>"]` if an open issue matches (`gh issue list`), otherwise `#[ignore = "feature: <one line>"]`.
- `greem`, `greem-build`, `greem-macros`, `greem-core` and `greem-reference` are out of bounds. A sync is test-only; fixes to the executor are the reviewer's to file.

## The pull request

Draft, titled `sync graphql-js <tag>`. The body uses this shape and ends with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`:

```markdown
## Summary

<pin -> tag; per suite: cases added / changed / removed, with the upstream titles>

## Evidence

<the audit output at the tag; the test, clippy and fmt results; for a check that
failed, its output verbatim>

## Needs a decision

<divergences, each with the test name and one line; feature-blocked cases without an
issue; upstream files added or removed, with what was done and why>

## Merge Danger

**Door:** two-way

**Blast Radius:** compliance tests
```

Open the pull request even when a check fails, with the failing output under Evidence, so the reviewer sees the state as it is.
