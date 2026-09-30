# Parked chains bound execution depth to the document, not to the data

The retained-frame ownership model nests one frame per generation, and polling and destruction recurse through that nest, so its length must be bounded. The length is not the tree depth: each `@defer` level adds a generation, every stream turn is a generation, and mutation root chains serialized together at the end nest inside one another. We bound it by changing what nests rather than by capping generations: a chain of frames that has finished its work parks, holding its storage, and retires after its last payload ships. Stream turns become sibling chains under a driver in the frame that produced the `Streamed` output; mutation root fields become sibling chains under a root driver; a per-request barrier handle steps every chain in lockstep. Depth is then a static property of the operation, checked at tree build against `Schema::max_depth` (default 32), and stream length and root-field count no longer touch it.

## Considered options

- A cap on the total generation count including turns: no design change, but stream length becomes `capacity × (limit − depth)` and batch mutations hit the limit, both failing at runtime after the initial payload may have shipped. A limit chosen for stack safety would be dictating data size.
- A static depth check plus a runtime generation cap only as a safety net: same failure for long streams, with two limits to explain.
- Free-running chains with payload emission at the outer barrier: simpler stepping, but payload contents per barrier would depend on interleaving, breaking the determinism properties the compliance harness relies on.

## Consequences

The executor is one barrier over several chains instead of one loop. Result storage for a chain's slots is owned by the chain, since leaf values borrow objects in its frames, so a request-wide `'req` value arena is not available. Retiring turn chains early means memory scales with in-flight work, not stream length. Group HALT and failure drop the group's chains at the barrier; cancellation and unwinding remain a single poll tree with RAII ordering. Depth evidence is a compliance concern: at-limit and 2×-default runs on a 2 MiB debug stack, plus tests that turns and roots add no depth.

The streamed-output proof refines the mechanism to [inspectable owner/dependent frames](0006-inspectable-frames-preserve-borrowed-payloads.md), preserving a structured borrowed payload across sibling chains. Its depth-64 completion, cancellation, unwind, 1,000-item stream and 1,000-root mutation probes pass on a 2 MiB debug stack; the walking skeleton must carry those checks into its real executor.
