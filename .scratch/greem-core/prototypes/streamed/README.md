# THROWAWAY: borrowed streaming through inspectable retained frames

The prototype answers **Does a Streamed output survive retained generation frames?**
Yes: the per-field resolver / Outputs contract composes with borrowing streams,
sibling turn chains, bounded pumping, cancellation, and a structured borrowed
payload sink. Preserving that sink requires inspectable owner/dependent frames,
rather than hiding every batch inside an opaque async frame future.

This is compiler and lifecycle evidence, **not a GraphQL server or the walking
skeleton**. Generated adapters are handwritten equivalents. The artifact emits
turn traces and asserts their ownership/scheduling invariants.

Captured on branch `codex/prototype/streamed-outputs`. The owning ticket records
its commit and the design resolution.

## Run

From the repository root, using Rust 1.98.0 and the committed lockfile:

```sh
cargo run --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin integrated
cargo run --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin depth
```

The second command runs each scenario on a **2 MiB thread stack in debug mode**.
Threads belong only to the stack-size harness; execution never spawns work and
uses `futures::executor`. On a machine without the cached dependencies, run
`cargo fetch --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml`
first. The dependencies are futures, serde, serde_json, and self_cell.

## What is proved together

| Requirement | Evidence |
| --- | --- |
| Actual per-field trait contract | `contract.rs` uses the accepted split `'obj`/`'call` Resolver signature and a GAT output bound on `Outputs<Field::Type, C>`. One blanket bridge delegates to tag-side `Completes`. |
| Cross-crate generated completion | The runtime is a library; the schema/application is the separate `integrated` binary. Object completion is generic over both the application type and context, constrained by the required field resolver traits. |
| Borrowed streams | One set-based parent resolver returns one `Streamed` per parent. Items borrow owned parent strings; nested detail objects borrow item strings. No application type map or cloned leaf values. |
| Invariant lifetimes / Send-only batches | Items contain `Mutex<&str>`; generated object batches contain `Cell<usize>`. The fully composed request future is checked as `Send`. |
| Owned continuations | A consuming `Box<Continuation>::run` returns completed items and the next owned continuation. Earlier borrowed items remain live across later runs. The driver's lower-level pump polls the same representation alongside active chains. |
| Initial items | Counts 0, 1, and greater than EOF; a disabled-streaming request drains inside its initial group. Continuations do not advance past the initial count until initial data actually ships. |
| Independent parents | Parents have different readiness; an assertion observes fast-parent data in an earlier payload than the matching slow-parent item. Initial completion batches both parents together. |
| Pump and capacity | Buffer capacity is asserted on every incremental pump pass; evidence counts buffering while a child resolver remains Pending. |
| Vec under streaming | The list tag adapts `Vec` into an always-ready stream through the same driver. |
| Item failure | A non-null error after initial delivery drops that source, suppresses failed-group data and descendants, and preserves the other stream and its defers. |
| Nested defer | A deferred scope receives its own group identity and frame after its parent payload ships. Its resolver returns a borrowing composite object, which gets a further frame and field resolver. |
| HALT | Failure in a streamed turn suppresses only that group and its descendants. Failure in one nested deferred group leaves its parent stream and the other deferred groups running. |
| Structured sink | `FnMut(Payload<'_>)` sees ordinary records containing borrowed strings from multiple independent frames. No encoded-fragment staging or owned JSON value tree. |
| Retirement / long stream | 1,000 items per parent with deferred fields: 4,000 records, at most five live turn chains, three nested frames below the request root. |
| Cancellation / panic | Cancellation before and after initial delivery, resolver panic, and the earlier sink-panic probe all check child-before-parent destruction. |
| Depth | Completion at 32 and 64; cancellation and deepest-resolver panic at 64, all on a 2 MiB debug stack. Every descendant destructor reads its still-live ancestor. |
| Turns do not add depth | 1,000 lazy items through depth-64 trees; maximum 63 simultaneous turns in that deep pipeline, independent of stream length. |
| Mutation roots do not add depth | 1,000 depth-64 mutation roots execute serially, park as siblings, and contribute to one final borrowed payload before any root storage drops. |

Outputs: [integrated runs](integrated-output.txt), [depth runs](depth-output.txt).
The corresponding `*-stderr.txt` files include deliberate, caught panics from
the harness; those commands still exit successfully.

## Frame and completion mechanism

`borrowed.rs` owns each output batch in a `self_cell` container and keeps its
borrowing chain as the dependent. The driver polls all active chains to the
barrier. It then borrows their ready views into one structured payload, calls
the synchronous sink, ends those borrows, and advances/retires the chains.

Normal async resolver futures remain inside those inspectable chains. Their
results move into owned completion columns. The consuming internal
`Completes::complete` step can move streams into continuations; generated object
columns then project borrowed object sets for their child resolvers. This changes
the old internal completion sketch, not the user's per-field output bound.

`Streamed<S>` has an internal defaulted item-type parameter with an owning
`PhantomData`. This gives the consuming completion method an explicit item
outlives relationship. `S: 'a` alone does not establish that `S::Item: 'a`;
leaving the item hidden behind an associated-type projection failed compilation.
Users construct the wrapper with `Streamed::new(source)` and retain the normal
`Streamed<S>` spelling.

The runtime uses the safe API of [self_cell 1.2.2](https://docs.rs/self_cell/1.2.2/self_cell/macro.self_cell.html),
including non-covariant dependent access. All handwritten probe code forbids
unsafe. The dependency and its macro expansion implement self-references with
unsafe code and stable heap storage; this is not a claim of an implementation
without unsafe dependencies. Owners drop only after their dependents.

This mechanism supersedes the literal `FuturesUnordered<BoxFuture>` storage
proposal: the current driver scans inspectable sibling chains. A ready queue
can optimize polling later, but must preserve accessible parked storage, the
global barrier, and a structured view across chains. Stream length never becomes
retained nesting depth.

## Compiler rejection evidence

These commands are expected to fail:

```sh
cargo rustc --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin integrated -- --cfg missing
cargo rustc --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin integrated -- --cfg wrong
cargo rustc --offline --manifest-path .scratch/greem-core/prototypes/streamed/Cargo.toml --bin borrowed -- --cfg escape
```

- [Missing resolver](missing-error.txt): the referring list output cannot satisfy
  its Outputs bound without the required item-field resolver.
- [Wrong output](wrong-error.txt): `i32` cannot complete as the field's text type.
- [Escaping response](escape-error.txt): E0521 rejects retaining a payload string
  outside the sink callback.

## Deliberate limits

- No SDL parser, executable validator, actual code generator, HTTP transport,
  production arena, null-propagation pass, error paths, or full GraphQL wire
  formatting. Those remain walking-skeleton work.
- Failure flags model the specific non-null/HALT delivery-boundary cases needed
  here. This is not the complete three-mode error-behavior implementation or a
  substitute for compliance fixtures.
- A mixed-group output batch can retain an unused failed-group value until its
  surviving dependents retire. Failed-group records are suppressed and later
  field work is filtered; shared ownership is not freed prematurely.
- The trace callback can run at empty generation barriers. The production sink
  emits only actual GraphQL payloads. Initial release is checked against data
  publication, not merely passage through an empty barrier.
- Generation bookkeeping in this probe is intentionally unoptimized. It proves
  the barrier/lifetime arrangement, not throughput or fairness under every source.
- No Miri run, no MSRV-floor run, and no claim that depth 64 is safe for arbitrary
  user resolver stack use. The measured evidence supports default 32 with the
  agreed 2× debug-stack margin for this execution machinery.

## Earlier alternatives

[Historical seam notes](seam-notes.md) preserve the initial investigation.
`cargo run` without a bin selects the original encoded-fragment probe; `--bin
borrowed` selects the earlier minimal structured-sink probe. The direct borrowed
registry fails in [borrowed-registry-fails.rs](borrowed-registry-fails.rs). These
are comparison artifacts, not the chosen implementation.
