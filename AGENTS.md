## Agent skills

### Issue tracker

Issues and specs live in this repo's GitHub Issues, managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles use their default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Formatting

`cargo fmt --all` formats Rust and TOML; `pnpm oxfmt` formats everything else. CI checks both.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### graphql-js sync

The ported graphql-js suites in `greem-compliance/tests/graphql_js` follow the tag pinned in `greem-compliance/Cargo.toml`. `.github/workflows/graphql-js-sync.yml` polls upstream and drafts the sync. See `docs/agents/graphql-js-sync.md`.
