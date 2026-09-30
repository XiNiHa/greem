# Does a Streamed output survive retained generation frames?

Type: prototype
Status: resolved
Assignee: Iha Shin
Blocked by: 07, 10, 17
Map: ../map.md

## Question

Prove, with the same cross-crate and cancellation bar as [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md), the stream design from [How do @defer and @stream map onto generations and the arena?](10-incremental-delivery-in-generations.md): a `Streamed<S>` list output whose items borrow `'obj` ancestors is moved into an owned continuation scope (`run(self: Box<Self>, …) -> (completed items, Option<Box<Self>>)`), polled by a capacity-bounded pump joined into every generation, has its completed items retained in the current frame's owned output batch while later turns continue, and is cancelled mid-stream with child-before-parent destruction. No spawning; runtime-agnostic.

Cover: `initialCount` items pulled inside the immediate scope; two parents in one scope with streams of different pace (turns must not lockstep); a `Vec` under `@stream` as an always-ready stream; a non-null item error terminating the stream and dropping the source; a nested `@defer` inside a streamed item; HALT inside a turn discarding that group only. The artifact serializes a turn trace, not the production arena. Link the artifact as an asset; the walking skeleton (14) blocks on this.

From [What execution-depth limits bound retained generation frames?](17-generation-depth-limits.md): turns are no longer appended to the request-wide frame chain. Prove instead a *stream driver* future in the frame where the `Streamed` output appeared, owning each turn's subtree as a sibling *chain* in a `FuturesUnordered<BoxFuture<'obj>>`; a per-request `Barrier` handle that frames await after each generation (arrive, Pending) and the top-level future advances once every live chain has arrived, keeping payload batching per barrier; a chain that *parks* when its work is done and *retires* at the first barrier after its turn's payload shipped, so a 1,000-turn stream holds only live turns; mutation root chains as parked siblings of a root driver, so depth is the max over roots; HALT dropping only the halted group's chains at the barrier. Assert with a depth counter that chain depth stays at the static execution depth across turns and roots.

## Comments

### 2026-09-28 — claimed; payload-assembly seam under review

Asset: [Streamed-output ownership probes](../prototypes/streamed/README.md).

A direct request-lifetime registry of references to chain-future-local owners
fails with E0597. A cross-crate positive probe passes with a two-phase barrier:
all chains arrive; each chain serializes its own borrowed data into owned JSON
fragments; the top level combines one payload and calls the sink; only then do
chains advance or retire. Two sibling chains retain owners across two payloads;
normal completion and cancellation drop children before items before parents.
This proves a safe assembly mechanism, not the full stream design, and the
negative probe does not rule out other borrowed sink implementations.

Pending human decision: adopt encoded fragments/bytes for payload assembly,
amending the earlier borrowed `Payload` sink design, or investigate a different
ownership mechanism preserving that sink. No resolution has been recorded.
The asset lists the remaining stream, HALT, retirement, mutation, and depth cases.

### 2026-09-28 — borrowed-payload alternative succeeds

Iha Shin requested trying the alternative that preserves a structured borrowed
`Payload` sink. The [new cross-crate probe](../prototypes/streamed/seam-notes.md#borrowed-payload-alternative--successful-probe)
does so using inspectable owner/dependent frames backed by `self_cell` 1.2.2.
Each frame owns its output batch beside the chain/futures borrowing it. Once all
chains arrive, the driver borrows their records into one `Payload<'view>`, invokes
the higher-ranked sink once, and only then advances/retires frames. No encoded
fragment staging, JSON value tree, or cloned leaf strings.

Evidence includes invariant-lifetime items; Send/non-Sync batches; repeated
serialization of the same structured view; actual streams with initial counts
0, 1 and beyond EOF; 1,000 items per parent with at most two live turn frames and
measured depth two; cancellation before/after initial delivery; sink panic
unwinding; child-before-parent destruction; and an E0521 negative compile probe
against escaping a payload borrow.

Tradeoff: this changes the previously proposed opaque async frames and literal
`FuturesUnordered<BoxFuture>` driver to inspectable owner/dependent storage with
a custom poll traversal. Handwritten code is safe; the dependency implements
self-references using unsafe. This is a candidate mechanism, not an accepted
map amendment. Full Outputs integration, errors/HALT, nested deferred execution,
mutation roots and deep stack evidence remain. No ticket resolution yet.

## Answer

Resolved 2026-09-28 after Iha Shin chose to pursue the structured borrowed-payload
alternative and instructed completion of the remaining trait, stream and
lifecycle proof. **The design works with inspectable owner/dependent frames.**
Keep the borrowed sink; replace the opaque async-frame storage sketch with the
proved frame mechanism below.

### The resulting contract

1. **Public resolver contract survives.** The cross-crate probe uses the earlier
   `Resolver<F, C>` signature with separate object/call lifetimes, GAT outputs,
   and the single `Outputs<Ty, C>` → tag-side `Completes<T, C>` bridge. Generated
   object completion is generic over both the application Rust type and context,
   bounded on required field resolvers. Missing item resolvers and wrong scalar
   outputs fail compilation at the referring output contract. No application
   type mapping or stream-only `'static` restriction.
2. **Completion consumes, then projects.** Internal completion takes owned
   output sets so list completion can move streams into continuations. Generated
   object columns retain their values and project borrowed child sets. The
   `Streamed<S>` wrapper carries an internal, defaulted item-type parameter with
   an owning phantom marker: `S: 'obj` alone does not establish the required
   item lifetime. The normal spelling and `Streamed::new(source)` stay simple.
3. **Inspectable generation frames.** Each frame owns an erased output batch
   beside a dependent chain that may borrow it, implemented through the safe API
   of `self_cell` (1.2.2 in the probe). This supports invariant item lifetimes and
   Send-only, non-Sync batches; generated projections borrow Sync objects into
   Send resolver futures. Handwritten code forbids unsafe, but the container
   dependency and its macro expansion implement self-references using unsafe.
   Stable heap storage and this dependency are the cost of preserving the sink.
4. **One barrier and one structured view.** Poll all active chains to the
   barrier, borrow their ready records into `Payload<'view>`, invoke the
   higher-ranked synchronous sink, end the response borrows, then advance or
   retire frames. No encoded-fragment staging, cloned leaf strings or owned JSON
   value tree. E0521 rejects a payload string escaping its callback. The literal
   `FuturesUnordered<BoxFuture>` ownership proposal is superseded by inspectable
   chains; ordinary async resolver futures still live inside them. A later ready
   queue can optimize polling without changing this ownership contract.
5. **Streams retain the proposed behavior.** Owned continuation `run(self:
   Box<Self>)` returns completed borrowed items and an optional next continuation;
   its bounded pump is also polled while generation work is pending. Initial
   counts are exact up to EOF, disabled streaming drains in the initial group,
   parents need not progress together, and a Vec enters the same driver as an
   always-ready stream. Incremental work releases after its parent's data ships,
   not after an empty generation barrier. The latter distinction caught and
   fixed a prototype bug.
6. **Group-local failure and deferred work.** A non-null item error terminates
   its source. A field HALT suppresses that stream group and its descendant
   groups while unrelated streams continue. Each deferred instance receives its
   own group identity; HALT in one deferred group leaves its parent stream and
   siblings alive. A real deferred scope/frame resolves a borrowing composite
   object, whose child field gets another retained frame. Failed-group work is
   filtered before later resolver invocations. A shared mixed-group owner batch
   may retain an unused value until its surviving dependents retire; it cannot
   be freed while they borrow it.
7. **Chains park and retire as siblings.** Turn storage retires after the last
   payload that reads it. Mutation roots execute serially and park as siblings
   until their single final borrowed payload is serialized. Their counts add
   width, not retained frame depth. Whole-request cancellation is dropping the
   one request future; dependents drop before owners on cancellation and unwind.

### Evidence

- Integrated typed run: two parents resolved together; borrowing Streamed and
  Vec outputs; initial counts 0, 1, and beyond EOF; disabled streaming; bounded
  pumping while pending; independently paced parents; nested defer; non-null
  source error; stream-group HALT; deferred-group HALT; cancellation before/after
  initial data; resolver panic. A 1,000-item-per-parent run emits 4,000 normal and
  deferred records with at most five live turn chains and three frames below the
  request root.
- Depth harness: debug completion at 32 and 64, cancellation at the deepest
  frame at 64, and deepest-resolver panic at 64, each on a 2 MiB stack. A 1,000-item
  lazy stream stays at depth 64 (at most 63 live turns in that deep pipeline).
  A 1,000-root mutation stays at depth 64, retains all parked roots and emits one
  final borrowed payload. Every descendant destructor reads its still-live
  ancestor; all drop-order assertions pass.
- Compile-fail probes: missing field resolver, mistyped scalar output and escaping
  payload borrow all reject for the expected reason. The earlier sink-panic and
  repeated-serialization seam checks also pass.

These are ownership, trait and scheduling probes. The trace is not GraphQL wire
output; the production arena, full error modes/null pass, error paths, tree-depth
validation, generated code, HTTP transport and compliance fixture suite belong
to **Does the skeleton run end-to-end?** The probe's empty barrier callbacks are
trace events; the production sink emits only actual payloads. Miri and the MSRV
floor were not run. Depth evidence does not promise arbitrary user resolver code
will fit the same stack.

### Assets and amendments

- [Runnable prototype and evidence matrix](../prototypes/streamed/README.md).
- [Integrated run trace](../prototypes/streamed/integrated-output.txt) and
  [bounded-stack results](../prototypes/streamed/depth-output.txt).
- Snapshot branch: `codex/prototype/streamed-outputs`, commit
  `e33dae262063cd65b2dd6908e5e6267237b9bee5` (prototype files only; main and the
  working index were not changed by capture).
- [ADR: inspectable retained frames preserve borrowed payloads](../../../docs/adr/0006-inspectable-frames-preserve-borrowed-payloads.md).

This refines the retained-frame and stream-driver mechanisms in the earlier
Outputs, incremental-delivery and execution-depth decisions. The structured sink,
global barrier, per-chain storage, stream/retirement semantics, and default depth
policy remain. The walking-skeleton ticket carries these implementation
requirements and is now unblocked. No new decision ticket is required.
