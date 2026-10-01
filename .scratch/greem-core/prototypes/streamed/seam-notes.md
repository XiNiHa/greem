> Historical investigation notes. The current verdict and evidence are in [README.md](README.md). Pending-work statements below describe earlier stages.

# THROWAWAY: streamed-output ownership probes

Status: **partial; the requested borrowed-payload alternative compiles and runs**. This is an
asset for [Does a Streamed output survive retained generation frames?](https://github.com/XiNiHa/greem/issues/20),
not the completed stream prototype or the walking skeleton. Neither ownership
alternative has been adopted as a map decision yet.

## Borrowed-payload alternative — successful probe

Requested by Iha Shin after reviewing the encoded-fragment approach below.

```sh
cargo run --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin borrowed
```

The runtime exposes an ordinary structured `Payload<'view>` to a higher-ranked
`FnMut` sink. Its records contain actual borrowed `&str` values from independent
turn frames. The sink inspects fields and serializes the same view twice, checking
identical output. No leaf strings, JSON values, or encoded fragments are copied
into a response registry. The payload allocates only its vector of view records;
the sink chooses when and how to encode it.

### What changed internally

Instead of keeping storage inaccessible inside an opaque async chain future,
each `Frame` is an **inspectable owner/dependent container**:

- Its owner is an erased output batch (`Box<dyn Batch + 'owner>`).
- Its dependent is an erased chain borrowing that batch. It may own pending
  resolver futures, completed child outputs, and sibling child frames.
- The driver polls work to the global barrier, then temporarily borrows every
  ready chain to build one structured payload. It invokes the sink once and
  ends all response borrows before advancing or retiring any frame.
- Resolver futures remain normal `Send` futures. Whole-request cancellation
  drops the frame tree, including pending child futures, before its ancestors.

[`self_cell` 1.2.2](https://docs.rs/self_cell/1.2.2/self_cell/macro.self_cell.html)
supplies the owner/dependent container. Its `not_covariant` scoped accessors
allow the dependent chain to be lifetime-invariant. All handwritten probe code
forbids unsafe; the dependency and its macro expansion contain the unsafe
implementation. This is **not an entirely safe-Rust implementation without an
unsafe dependency**, and is not a new arena or a reverse-drop-order assumption.
The dependency's construction/access/drop contracts carry that responsibility.

The test application deliberately uses `Mutex<&'parent str>` inside items to
make their lifetime invariant, and `Cell<usize>` in batches so the batch is
`Send` but not `Sync`. Child futures project only Sync item references. Keeping
the child's view lifetime separate from the item's parent lifetime is necessary;
conflating them was rejected by rustc.

### Evidence

Runtime and application are separate library/binary crates. On Rust 1.98.0:

- Two borrowing streams with different readiness; exact initial sets with
  `initialCount` 0, 1, and greater than the source length.
- Capacity-one pumping while child work is pending, sibling turn retention
  through a later deferred-data phase, and retirement after the sink returns.
- 1,000 items **per parent**, delivered over 2,000 barriers in this fixture:
  at most two live turn frames, measured frame-tree depth two throughout.
  This is width/length evidence, **not** the separate depth-64 stack proof.
- Cancellation before the first payload and after the first payload with later
  work pending. Child destructors read item data; item destructors read parent
  data. Assertions verify child-before-item-before-parent order.
- Intentional sink panic and unwinding with the same destruction checks. The
  panic is caught by the probe harness only, not by the runtime.
- A negative compile probe rejects saving a borrowed payload field outside the
  callback with E0521.

Outputs: [successful runs](borrowed-run-output.txt),
[build log and intentional panic](borrowed-run-stderr.txt), and
[borrow escape diagnostic](borrowed-payload-escape-error.txt).

```sh
# Expected compile failure:
cargo rustc --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin borrowed -- --cfg escape
```

### Costs and limits

This retains the borrowed sink but changes the earlier async-frame mechanism:
owner/dependent containers add stable heap storage and an unsafe dependency.
The current driver scans inspectable sibling frames; it does **not** prove the
earlier literal `FuturesUnordered<BoxFuture>` arrangement. A future scheduler
could separate runnable-future indexing from retained frame storage; it still
must expose the parked views and preserve barrier readiness.

The trace is not GraphQL wire output. The generated adapter is handwritten;
the actual `Resolver`/`Outputs` integration is still outstanding. The later
deferred-data phase proves retention across payloads, not full nested `@defer`
execution. HALT, item failures, mutation root chains, a complete arena and deep
stack/unwind stress are not established. Miri was not run (not installed).

The successful alternative means encoded fragments are **not necessary** to
obtain a safe-to-use borrowed sink. Adopting this frame mechanism still needs
the map's human review; the ticket remains claimed.

## First question: who can serialize chain-owned data?

The parked-chain design leaves each output owner inside a suspended future.
A top-level barrier cannot simply register references to those local owners in
a request-lifetime registry. `borrowed-registry-fails.rs` isolates that attempt;
Rust rejects it with E0597. This rules out that direct registry, **not every
possible borrowed sink design**.

The positive probe uses a two-phase barrier:

1. Every live chain resolves and arrives, retaining its output owners.
2. The barrier invites each chain to serialize its local borrowed fragment.
3. Each chain deposits owned JSON bytes, not references or `serde_json::Value`.
4. The barrier combines fragments in registration order and calls the sink once.
5. Only after the sink returns does the barrier advance. Chains can then run
   deferred work or retire, dropping descendants before their owners.

Runtime and application are distinct library/binary crates. The application owns
parents in its root future; sibling chains own items borrowing those parents;
children borrow those items. Fragment fields are `&str` into those owners.
There is no unsafe code, spawn, runtime dependency, leaf clone, or leaked owner.
`BoxFuture` and the runtime's bound check that the futures are `Send`.

The output is a **turn-shaped trace**, not GraphQL wire output. JSON is encoded
locally once, then copied into the envelope. The proof does not establish that
arbitrary GraphQL payload assembly can concatenate fragments without additional
metadata, nor preserve the existing public `FnMut(Payload<'_>)` contract. That
contract was the pending decision at this stage. The alternative above now
demonstrates one way to preserve it by changing how frames are represented.

## Run

From the repository root, using Rust 1.98.0 and cached dependencies:

```sh
cargo run --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml
```

Assertions demonstrate one combined payload for both sibling chains at each
barrier, owners still alive during both sink calls, a pending child, and
child-before-item-before-parent destruction on completion and cancellation.
`run-output.txt` records the result.

The negative probe is expected to fail:

```sh
rustc --edition 2024 .scratch/greem-core/prototypes/streamed/borrowed-registry-fails.rs
```

`borrowed-registry-error.txt` records its compiler diagnostic.

## Still required before resolving the ticket

- Actual `Streamed<S>` outputs and their generated `Outputs`/`Completes` bridge.
- The precise owned continuation-scope API and integration with generated
  completion. The alternative now exercises initial counts and bounded pumping
  with actual streams, but uses a custom inspectable driver.
- Always-ready `Vec` streams, item failures, source destruction, and HALT scoped
  to one delivery group.
- Real nested deferred scopes; the present second barrier only models their
  storage retention and release sequence.
- Sibling mutation root chains and depth evidence across a generated frame tree.
  Turn retirement and length-independent depth are now demonstrated by the
  alternative's fixed-depth, 1,000-items-per-parent fixture.
- Cancellation with live sources and overlapping turns, plus the full stress
  evidence required by the depth decision.

No map decision or accepted sink contract has been changed. Capture the validated
prototype on a throwaway branch after the human review and remaining evidence.
