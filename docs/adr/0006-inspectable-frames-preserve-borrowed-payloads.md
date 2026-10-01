# Inspectable retained frames preserve the borrowed payload sink

A request-wide registry cannot retain references to output owners hidden inside suspended chain futures. Keep the structured `FnMut(Payload<'_>)` sink by storing each batch and its borrowing chain together in an inspectable owner/dependent frame, using the safe API of `self_cell`; the barrier borrows all ready views, calls the sink once, then advances or retires frames after those borrows end. The cross-crate streamed-output prototype proves this with the per-field Resolver/Outputs contract, invariant item lifetimes, nested deferred output, cancellation and bounded-depth execution.

## Considered options

Encoding each chain into owned JSON fragments before assembling a payload also worked, but changes the response boundary into encoded data. Iha Shin chose to pursue the borrowed-sink alternative and requested the remaining stream and lifecycle proof. A direct borrowed registry failed compilation; we did not infer from that failure that every borrowed sink was impossible.

## Consequences

The earlier literal `FuturesUnordered<BoxFuture>` ownership sketch is replaced by inspectable chains with ordinary resolver futures inside them. Frames require stable heap storage and an unsafe dependency implementation; handwritten runtime/generated code uses safe interfaces. Streams enter owned continuations through consuming internal completion, while generated object batches project borrowed child sets. Stream turns and mutation roots remain siblings, preserving the depth policy in [Parked chains bound execution depth](0005-parked-chains-bound-execution-depth.md).

The evidence and precise amendments live in [Does a Streamed output survive retained generation frames?](https://github.com/XiNiHa/greem/issues/20). This does not establish a production arena, full GraphQL error semantics, or the walking skeleton.
