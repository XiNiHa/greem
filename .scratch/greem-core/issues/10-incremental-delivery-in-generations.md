# How do @defer and @stream map onto generations and the arena?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 03, 08
Map: ../map.md

## Question

Design, not implement: a deferred fragment as a scope subtree scheduled after the initial result is serialized; @stream against list flattening (initialCount, items delivered later); how the arena supports serializing a subtree now and patching later; how pending/incremental/completed payloads and ids derive from scopes; interaction with post-hoc null propagation. Output: constraints on 08's data model that the skeleton must honor so this can land later.

From ticket 08: the result arena is columnar per scope with bidirectional links, so a scope subtree serializes alone via the same `Response<'req>: Serialize` view; error paths are lazy (re-rootable per payload). The execution tree is per request and data-free, so a deferred fragment is a set of tree nodes whose scopes are simply enqueued into a later generation. Check whether `Items(range)` list shapes suffice for `@stream`'s partial delivery or need a "delivered up to" mark.

From [How does post-hoc null propagation and onError work over the arena?](09-nullability-and-onerror-semantics.md): the pass is subtree-rooted (already used per mutation root chain), so a deferred fragment's payload gets its own pass rooted at the fragment position; slots nulled by propagation are `Propagated(id)`, distinct from intentional `Null`, which is the signal to drop incremental work beneath them and to suppress an entry into `completed.errors` when a walk crosses the fragment/list boundary. `ErrorBehavior` exists in the executor; define HALT's unit as the incremental result here.

## Answer

Agreed with Iha Shin, 2026-09-26. Nineteen questions; all resolved as recommended except the two below where Iha chose differently: a list resolver may return a lazy stream from day one (Q3), and incremental delivery is **in scope for the skeleton** (Q17), which redraws the map's destination.

**Vocabulary** (now in `CONTEXT.md`): a *delivery group* is one unit the client sees as pending and later completed (a deferred fragment at one concrete object, or a streamed list at one concrete parent); the *initial group* is everything in the first payload; *release* is the moment a group's parent payload has shipped; a *stream turn* is one batch of streamed items completed together in one generation; the *delivery boundary* is the response position at a group's root.

### Scheduling and scopes

1. **One loop, tagged scopes.** Every scope carries its delivery group. The generation barrier stays global: scopes of different groups in one generation run concurrently. After each barrier the executor emits payloads for every group with no live scopes left. Deferred scopes are parked until release (no early execution; an "enqueue immediately" flag is fog, and needs no data-model change). Rejected: a nested loop per fragment (duplicates the loop, breaks set-based batching under lists); early execution by default.
2. **A deferred fragment is a separate scope over the same objects** at the same tree node. Scope identity becomes (parent scope, node, partition leaf, delivery group). The immediate scope runs only the non-deferred fields. Consequence for codegen: the per-(node, type) Plan splits its grouped field set by delivery group and generated `Scope::run` runs one group's fields; the retained ancestor frames are alive anyway, so the deferred scope borrows them for free.
3. **Overlapping fragments follow the spec's no-duplication rule.** The tag is a *set* of defer usages; a fragment instance completes when every group containing it has completed (one counter per instance); the incremental entry is attributed to the pending id with the longest path and carries `subPath`. Merging overlapping fragments at one position into one pending entry is a legal optimization, not the design.
4. **Release.** A group's root scopes are enqueued into the generation after the barrier that emitted its parent group's payload, and only if the parent position survived that payload's null pass. A group under a list is one instance per object but one scope per set; with barrier batching all instances' parents ship at the same barrier, so one scope stands for N instances.

### Streams

5. **`Streamed<S>` wrapper**, `S: Stream<Item = Result<T, Error>> + Send`, with a generated `Completes` impl at list positions only, the same idiom as member wrappers (no blanket over `Stream`, no field-marker opt-in). The resolver chooses per field whether its list is lazy; when the request does not `@stream` that field the executor drains it inside the immediate scope and it completes like a `Vec`. A `Vec` under `@stream` is an always-ready stream. `initialCount` items are pulled inside the immediate scope, exactly `initialCount` per parent. An `Err` item is an execution error at `[i]`: nullable item type → `null` in `items` plus the error; non-null item type → the stream fails (`completed.errors`, source dropped). There is no separate whole-stream failure.
6. **Turns.** After the immediate scope, the live streams of a scope move into a continuation. Each turn awaits until at least one stream has an item ready, bounded by a batch capacity (default 100), completes whatever arrived across all parents as one set-based scope, and returns the next turn if any stream is live. Items are delivered in per-parent index order. Rejected: lockstep across parents; draining fully in one turn.
7. **Ownership: an owned continuation scope kind plus a pump; no spawning.** Continuations are `run(self: Box<Self>, …) -> (completed items, Option<Box<Self>>)`, moved from generation to generation, holding live streams by value; completed items land in the current frame's owned output batch and may borrow `'obj` ancestors. Live streams are also polled by a capacity-bounded pump future joined into *every* generation, so items buffer between turns. Tokio-only with `spawn` per stream was weighed and rejected: `tokio::spawn` needs `'static`, so streamed lists alone could not borrow their parents (an asymmetry in the resolver contract); cancellation would leave RAII for abort-on-drop guards; a `Receiver` still needs `&mut`, so the owned kind does not disappear; and it would reverse the runtime-agnostic standing decision. The pump gets the latency benefit; only multi-core parallelism is forgone, which a resolver can buy itself by spawning inside its own `'static` future. Proven by [Does a Streamed output survive retained generation frames?](18-streamed-output-prototype.md), on which the skeleton blocks.
8. **Arena.** A composite column keeps its initial child scope and `Items(range)` array from ticket 08 and gains `tails: Vec<TailLink>`, one per turn, each naming that turn's scope and carrying its own per-parent `Items(range)` array; serializing turn *t* for parent *p* reads that entry. Only the outermost list level has tails. Non-streamed columns have an empty tail list.

### Payloads

9. **Ids, pending, batching.** A delivery group instance is (defer usage, object) for `@defer` and (field node, parent object) for `@stream`. Ids come from one per-request counter, assigned at release in scope order then object order, stringified, hence deterministic. The barrier is the batch unit: each barrier emits at most one update result holding every `pending` (children of payloads shipped now), `incremental` and `completed` it produced; `hasNext` is false when no group is live. `pending` is one entry per instance (one per list item under a list, as the spec requires).
10. **Null propagation at the boundary.** The pass from ticket 09 takes an optional boundary. A walk that would rewrite a slot at or above the boundary instead marks the group *failed* with that error and rewrites nothing above; the mutation-root chains of ticket 09 pass no boundary. A failed group emits `completed.errors` and no `incremental` entry, and every group pending beneath it is dropped. At release, a group whose parent position is `Propagated`, `Error` or `Null` is dropped before it runs. Delivered data is never rewritten.
11. **Error behavior per group.** HALT in the initial group: `data: null`, exactly one error, no `pending`, a plain non-incremental response, all parked groups discarded. HALT inside a delivery group: that group completes with `errors: [first error]`, no `incremental`, its descendants dropped, unrelated groups continue. NULL: nothing propagates, a group never fails, errors ride on its incremental entry. PROPAGATE: item 10. Ticket 09's HALT flag is therefore observed per group at the barrier, not per request.
12. **Sink.** Ticket 07's `execute_with(finish)` generalizes to a sink `FnMut(Payload<'_>)` invoked at the barrier for each payload, synchronous, frames alive; the one-callback API is the single-payload special case. Adapters wrap it into an async stream of owned bytes (channel); back-pressure is theirs. Nothing frame-dependent escapes.
13. **Directives at tree build.** `@defer(if:)` and `@stream(if:)` are applied like `@skip`/`@include`, recorded as consumed condition variables for the future tree cache; `initialCount` and `label` are stored on the node. The executor takes `IncrementalDelivery { Disabled, Enabled }`; `Disabled` makes tree build ignore both directives, which is spec-legal ("servers may ignore any directive") and yields a plain response.
14. **Transport (axum example).** `multipart/mixed` per the graphql-over-http RFC: chunked on HTTP/1.1, no `Transfer-Encoding` on HTTP/2, `CRLF---CRLF` boundaries, `application/json` parts, payload shape from spec Section 7. `Enabled` iff the request's `Accept` lists `multipart/mixed`. Apollo's `deferSpec` parameter is ignored. SSE is fog.
15. **Compliance (ticket 13).** Two layers. Property: whenever the `Disabled` execution raises no error that propagates across a defer or stream boundary, folding the incremental stream (apply each incremental entry at `path` + `subPath`, append items in order) equals the `Disabled` BFS result, itself ≡ DFS; errors compare as sets of (path, message). Failed groups, nulled-parent cancellation and non-null stream termination are hand-written spec tests with graphql-js's test cases as fixtures. Rejected: teaching the reference executor incremental semantics.

### Scope

16. **Incremental delivery is in the skeleton.** One `@defer` and one `@stream` query run end-to-end through codegen, executor, sink and the axum example. The map's Destination and Out-of-scope sections are redrawn; the stream prototype (18) blocks the skeleton (14); tickets 12, 13 and 14 are amended. Fallback if 14 proves too large for one session once 12 and 13 resolve: split `@stream` out of the skeleton and let 18 stand as its proof.

**Data-model constraints, now all live in the skeleton:** delivery-group tag on every scope; defer-usage set and optional stream usage on tree nodes; the `IncrementalDelivery` switch; boundary parameter and hit-report on the null pass; `tails` on composite columns; the sink primitive; serialization rooted at a delivery group.

Extension points: early-execution flag; SSE transport; batch capacity tuning; barrier-less scheduling (fog); `onError` wire attribute (out of scope).

Assets: [ADR 0003 incremental delivery as tagged scopes](../../../docs/adr/0003-incremental-delivery-as-tagged-scopes.md).

### Amendment from [What execution-depth limits bound retained generation frames?](17-generation-depth-limits.md) — 2026-09-28

Item 7's "completed items land in the current frame's owned output batch" made every turn a new frame on the one request-wide chain, so frame depth grew with stream length. Turns now run as sibling chains owned by a stream driver in the frame where the `Streamed` output appeared; the single generation loop becomes one barrier handle stepping several chains in lockstep. Payload-per-barrier, release, boundary and HALT rules are unchanged.

### Mechanism amendment from [Does a Streamed output survive retained generation frames?](18-streamed-output-prototype.md)

The proved implementation uses inspectable owner/dependent frames to retain the
structured borrowed sink across sibling chains. Internal completion consumes
owned outputs before projecting object sets; stream continuations own their
sources and expose both consuming turns and bounded pumping. The public resolver
contract, no-spawn execution, group-local failure and one-payload-per-barrier
semantics remain. Release follows actual parent-payload publication, not an empty
generation barrier. See the stream ticket for the evidence and exact storage/API
amendments.
