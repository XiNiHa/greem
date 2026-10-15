# bench

The HTTP driver and the implementations it measures, over the workloads in `workloads/` (see `workloads/CONTRACT.md`). A separate Cargo workspace, so nothing here enters the root build.

```sh
cd bench
cargo run --release -p greem-bench-driver            # every implementation in servers.toml, every size
cargo run --release -p greem-bench-driver -- --sizes smallest --only greem --only graphql-js
cargo run --release -p greem-bench-driver -- --help
```

The driver builds and starts each entry of `servers.toml` in turn, checks it against every workload's oracle, measures at each `--concurrency` level after `--warmup` requests per connection, prints the Markdown report and writes it as `results/<started>.md` beside `results/<started>.json`. `--server name=url` measures an entry already running elsewhere.

- `greem`: `greem-bench`'s `serve` binary, over the same `App` the criterion bench measures.
- `servers/async-graphql`: async-graphql behind `async-graphql-axum`.
- `servers/graphql-js`: graphql-js 17 behind graphql-http on `node:http`; incremental requests, which graphql-http cannot serve, run its `experimentalExecuteIncrementally` with the multipart framing written there. Node and pnpm are pinned in the root `mise.toml`; `bench/` is a pnpm workspace of its own, apart from the root's.
