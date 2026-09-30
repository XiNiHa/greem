# What execution-depth limits bound retained generation frames?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 07
Map: ../map.md

## Question

Set the depth/resource policy for safe retained generation frames. The integrated prototype has depth-dependent polling and destruction call chains, while the response remains borrowed until final serialization. Choose how supported operation depth is bounded and checked (including fragments, aliases and runtime abstract expansion), the configurable/default limit, how rejection is surfaced, and what stress evidence establishes a safe operating range. Cover normal execution, cancellation and unwinding. If unbounded depth is required, identify and validate a different ownership/scheduling mechanism before the skeleton relies on it.

Context: [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md). Preserve breadth-first generation barriers and the accepted borrowed-response boundary; this ticket settles limits and their validation, not general service quotas or benchmarking.

From [How does SDL become generated code, and how does the runtime get the schema?](12-build-pipeline-and-crate-layout.md): document-level limits (apollo's parser `recursion_limit` and `token_limit`, and `introspection::check_max_depth`) are `Schema` builder options with apollo's defaults and are checked in `Schema::parse` before any tree exists; this ticket sets the execution-depth policy on top of them, and should state whether the tree-build depth check (fragments expanded, aliases counted) subsumes the parser recursion limit or complements it.

From [How is the BFS executor shown spec-correct?](13-compliance-harness.md): the stress evidence this ticket asks for lands as `greem-compliance` tests using the world's gate resolver and drop log; the document generator can be given a depth knob so the limit's rejection path is property-tested rather than hand-picked.

From [How are eagerly built Plan entries stored and borrowed per request?](16-lazy-plan-storage.md): the planning walk is one recursive pass over the tree, so its recursion depth equals tree depth and the limit chosen here must cover it alongside the retained generation frames.

## Answer

Agreed with Iha Shin, 2026-09-28. Eleven questions over two rounds; all resolved as recommended.

**Vocabulary** (now in `CONTEXT.md`): a *generation frame* owns one generation's completed output batches while descendants borrow them; *execution depth* is the number of nested frames an operation can require, fixed at tree build; a *chain* is a nested run of frames rooted where its objects were produced, stepped in lockstep with the barrier; a chain *parks* when its work is done and *retires* when its storage drops.

### The finding that reshaped the question

The frame chain is one frame per generation, and the generation count is not the tree depth. Three sources stretch it: each nesting level of `@defer` adds one generation (release at barrier g runs at g+1); every stream turn is a generation, so a stream of N items at capacity 100 would add N/100 frames, data-dependent and unbounded by any document check; and mutation root chains, serialized once at the end while every owner is alive, would nest inside one another so depth *sums* across root fields. Bounding the tree depth alone would have covered only plain queries.

### Mechanism: parked chains, not a generation cap

1. **Chains.** A nested frame chain that finishes its generations does not return; it parks, holding its storage. Each stream turn's subtree is its own chain, a sibling owned by a *stream driver* future living in the frame where the `Streamed` output appeared (siblings in a `FuturesUnordered<BoxFuture<'obj>>` may be pushed while others run; they borrow ancestors, never each other). Each mutation root chain is likewise a parked sibling owned by a mutation driver at the root. Chain depth is therefore exactly the static execution depth, independent of stream length and root-field count. Rejected: a cap on total generations including turns (stream length becomes a function of the stack-safety limit, failing after the initial payload shipped); a hybrid static check plus runtime cap for streams.
2. **Lockstep.** A per-request `Barrier` handle is threaded into every frame. After joining its generation's scopes a frame awaits `barrier.wait(g)`, which marks the chain *arrived* and returns Pending. The top-level execute future polls the root chain; when the poll is Pending and every live chain has arrived (registered on creation, deregistered on park), it runs the barrier work (null pass for released groups, payload emission through the sink, group release, HALT flags), advances and re-polls. Each frame's join is its own scopes plus its owned chains. Still one poll tree, no spawning; ticket 13's determinism properties and ticket 10's one-update-result-per-barrier rule hold. Rejected: free-running chains with emission at the outer barrier.
3. **Retirement.** A chain parks when all work beneath it, including deferred groups released under it, is complete, and retires at the first barrier after the last payload that reads its data has shipped: a turn chain right after its turn's payload, mutation chains and everything in a single-payload request at the end. Memory scales with in-flight work, not stream length. **Requirement for the skeleton:** the result storage for a chain's slots is owned by the chain, not by a request-wide `'req` arena, since leaf values borrow objects in those frames; ticket 08's `Value<'req>` sketch must be reconciled with per-chain ownership.
4. **HALT, failure, cancellation, panics.** At the barrier a chain whose delivery group is halted or failed is dropped instead of advanced; RAII gives child-before-parent order, and a driver drops its chains before its frame's batches because it is declared after them. Whole-request cancellation is dropping the top-level future. A panic unwinds through the single poll call; no `catch_unwind` in the skeleton. Mid-generation cancellation stays in the fog.

### The limit

5. **Execution depth is checked at tree build** with a counter that fails fast while descending (fragment spread chains expand far past the syntactic nesting the parser saw, so the check cannot wait for the tree to exist). Rejection is a request error payload on the validation path, message naming the computed depth and the limit, no `data`. Counting rules: each nested selection set on a composite-typed field adds one; list nesting, inline fragments, spreads, aliases, abstract expansion and introspection nodes add none; `@defer` adds one where its `if` resolves true and incremental delivery is enabled; `@stream` adds one (the turn chain's first frame sits one level below the initial items' frame); mutations take the max over root chains. The reference executor shares the tree and inherits the check. Frames carry a `debug_assert` depth counter as a regression guard.
6. **Three independent knobs.** apollo's parser `recursion_limit` (default 500, syntactic nesting including input values) and `introspection::check_max_depth` (`ofType` chains, pre-resolved outside generations) stay as they are; the execution limit is the one users tune and sits well below 500 so tree-build recursion is bounded by it, not by the parser.
7. **`Schema` builder option `max_depth`, default 32, no hard ceiling.** Deployment policy, not per-request. Rejected: a field on `ExecuteOptions`. The docs state what was validated and on what stack size; raising it is the user's call.

### Evidence (lands in `greem-compliance`, executed by the skeleton)

8. The document generator gains a depth knob; a property asserts depth ≤ limit is never rejected and > limit always is, with the same outcome from the reference executor.
9. Hand-written tests run at exactly the limit and at 2× the default on a thread spawned with tokio's default worker stack size (2 MiB) in a debug build: completion; cancellation at the deepest generation through the gate resolver with the drop log ordered child-before-parent; a panicking deepest resolver unwinding with the same log. The documented number is "default × 2 passes in debug on a 2 MiB stack".
10. A 1,000-turn stream and a mutation with many root fields at max chain depth complete on that same stack, proving neither adds depth.
11. The reference executor recurses several Rust frames per level, so the harness spawns it on a thread with a larger explicit stack rather than shaping the limit around an oracle.

### Consequences for other tickets

- **Streamed prototype (18):** now proves the stream driver, sibling turn chains, the barrier handle and lockstep, parking, retirement after the turn's payload, and mutation root chains as siblings, under the same cross-crate and cancellation bar.
- **Walking skeleton (14):** per-chain result storage; `max_depth` on the builder; the counting rules; the compliance evidence above.
- **Incremental delivery (10):** "one generation loop" is amended to one barrier over several chains; the payload-per-barrier and release rules are unchanged.

Assets: [ADR 0005 parked chains bound execution depth](../../../docs/adr/0005-parked-chains-bound-execution-depth.md).

### Mechanism amendment from [Does a Streamed output survive retained generation frames?](18-streamed-output-prototype.md)

The stream proof establishes inspectable owner/dependent frames instead of
opaque async frames in `FuturesUnordered<BoxFuture>`, allowing one structured
borrowed payload to span sibling chains. The barrier, sibling ownership,
retirement rules and default depth policy remain. Its depth-64 completion,
cancellation, unwind, 1,000-item stream and 1,000-root mutation probes pass on a
2 MiB debug stack; the skeleton must repeat them on its actual implementation.
