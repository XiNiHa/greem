# What is the current incremental delivery (@defer/@stream) spec shape?

Type: research
Status: resolved
Blocked by: 
Map: ../map.md

## Question

Pin the current state of the incremental delivery RFC in graphql/graphql-spec and graphql/graphql-wg (primary sources; note which of the 2023+ redesigns is current). Capture: directive arguments (label, if, initialCount), the response format (pending / incremental / completed, ids, paths), delivery ordering guarantees, how a deferred fragment's errors and non-null propagation interact with the already-delivered parent, how @stream over lists interacts with nullability and with items delivered in the initial payload, the transport specs (incremental delivery over HTTP: multipart/mixed, SSE), and what graphql-js's current implementation does where the spec is still open.

## Answer

- Current proposal is graphql-spec PR #1110 (RFC2 since 2026-03-05), the "June 2023" no-duplication format from defer-stream-wg discussion #69; PR #742 and all 2023–2025 alternatives are closed/superseded. It lands in slices on the `incremental-integration` branch: Section 3 directives, Section 7 response + Appendix E, Section 5 validation are merged; Section 6 `CollectFields` (#1234) is under review; plan-generation/work-queue text exists only in #1110's head, and `@stream` execution text does not exist yet anywhere. `main` has no `@defer`.
- Directives: `@defer(if: Boolean! = true, label: String) on FRAGMENT_SPREAD | INLINE_FRAGMENT`, `@stream(if: Boolean! = true, label: String, initialCount: Int! = 0) on FIELD`; labels are literal-only and unique across both directives; `@skip`/`@include` win; servers may ignore any individual directive and must send at least `initialCount` items (never fewer) — clients must cope with both.
- Response: `data`+`pending`+`hasNext` (+`errors`/`incremental`/`completed`) initially, then `hasNext` (+`pending`/`incremental`/`completed`) updates with no top-level `data`/`errors`. `pending {id,path,label?}`; `incremental` is `{id, items}` for streams or `{id, subPath?, data, errors?}` for defers, attributed to one pending id (longest path) and never duplicating fields; `completed {id, errors?}` closes a fragment/stream. Ids are opaque unique strings.
- Ordering: a pending notice precedes/accompanies anything referencing it; nested work becomes pending only when its parent payload ships; a deferred fragment is delivered whole; stream items are index-ordered without indices; independent fragments/streams are unordered; batching payloads is allowed; early execution is implementation-defined.
- Errors: errors contained inside the deferred/streamed data ride on the incremental entry; a non-null error that would bubble to or past the fragment/list position suppresses the entry entirely and surfaces as `completed.errors` — the already-delivered parent is never rewritten. Nulled positions in the initial result drop all incremental work beneath them (graphql-js cancels it; the spec text only implies it).
- `@stream`: nullable item errors → `items:[null]` + `errors`; non-null item error after `initialCount` → stream terminated via `completed.errors`; errors before `initialCount` behave normally in the initial result; only the outermost list streams.
- Validation adds four rules plus "no two mergeable fields where either has `@stream`" (alias to get independent streams); defer/stream are forbidden on mutation/subscription root fields and must be `if`-disable-able in subscriptions.
- Transport: graphql-over-http's normative spec is silent; the `multipart/mixed` RFC (chunked on HTTP/1.1, no `Transfer-Encoding` on HTTP/2, `CRLF---CRLF` boundaries) is stale with legacy example payloads and no `Accept` negotiation; the SSE RFC only says closing the connection terminates `@defer`/`@stream`.
- graphql-js v17.0.x (stable since 2026-06-15) ships it behind `experimentalExecuteIncrementally` (map `{initialResult, subsequentResults}`), keeps `legacyExecuteIncrementally` for the old `path`/`label` format, throws from `execute()` if the schema defines the directives, honours `initialCount` exactly, defaults `enableEarlyExecution` off, batches stream items at 100, and its `incremental/` work-queue is what Section 6 transcribes.

Full write-up: [research/incremental-delivery-spec-state.md](../research/incremental-delivery-spec-state.md)
