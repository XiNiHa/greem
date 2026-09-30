# Incremental delivery as tagged scopes in the one generation loop, streams pumped inside the join

`@defer` and `@stream` are not a second executor. Every scope carries a delivery group; the existing generation barrier also emits payloads for groups that have no live scopes left; a deferred fragment is just another scope over the same objects, run after its parent payload ships. Lazy lists are an explicit `Streamed<S>` output whose live streams sit in owned continuation scopes moved from generation to generation and polled by a capacity-bounded pump joined into every generation. We do this instead of a graphql-js-style work queue or spawned tasks so that the set-based primitive, the retained-frame ownership model and the runtime-agnostic, no-spawn executor all survive unchanged.

## Considered options

- A work queue per deferred fragment / stream (graphql-js): duplicates the scheduling loop and loses set-based batching of one fragment across many list items.
- Tokio-only with `spawn` per stream: `tokio::spawn` requires `'static`, so streamed lists alone could not borrow their parent objects, an asymmetry in the resolver contract; cancellation would need abort-on-drop guards instead of RAII; and a channel receiver still needs `&mut`, so the owned continuation does not go away. The pump buys the same latency for I/O-bound streams; only multi-core parallelism is forgone, which a resolver can obtain itself.

## Consequences

Scope identity gains a delivery group; the per-(node, type) Plan splits by group and codegen emits one `Scope::run` per group. Composite columns carry per-turn tails. The null pass takes a boundary and fails a group rather than rewriting delivered data. The response primitive is a per-payload sink, of which the single callback is the one-payload case.
