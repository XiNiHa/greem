# Does the skeleton run end-to-end?

Type: prototype
Status: resolved
Assignee: Iha Shin
Blocked by: 05, 06, 07, 08, 09, 10, 11, 12, 13, 16, 17, 18
Map: ../map.md

## Question

Build the walking skeleton: SDL with an object type, a nested list, one interface, and one nullable field → greem-build codegen → per-field resolvers (one set-based, one per-object sugar) → BFS executor with generation barrier, arena, post-hoc null pass, minimal planning pass → one axum example → equivalence test against the reference executor. Resolution: the spec document that describes the core and points at the skeleton.

From [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md): use explicit per-field adapters, the reserved parent-error completion hook, safe retained generation frames, separate abstract partition-arm scopes, and callback-based borrowed response serialization. Its prototype is the compiler/lifetime evidence, not the skeleton. Wire the production columnar result arena and full error/cardinality handling, and apply the Plan-storage and depth-limit decisions before calling this complete.

From [How does post-hoc null propagation and onError work over the arena?](09-nullability-and-onerror-semantics.md): the executor takes `ErrorBehavior { Null, Propagate, Halt }` from day one; the null pass is the error-driven, subtree-rooted walk with the `Propagated(id)` slot variant; the axum example need not parse the `onError` request attribute.

From [How do @defer and @stream map onto generations and the arena?](10-incremental-delivery-in-generations.md): the skeleton also runs one `@defer` and one `@stream` query end-to-end: tagged scopes in one generation loop, release at the barrier, the boundary-aware null pass, deterministic ids, batching per barrier, `Streamed<S>` with owned continuations and the pump (proven first by [Does a Streamed output survive retained generation frames?](18-streamed-output-prototype.md)), the sink primitive, and `multipart/mixed` in the axum example with `Enabled` iff `Accept` lists `multipart/mixed`. If this proves too large for one session, split `@stream` out and let 18 stand as its proof.

From [What can a field say to its ancestors during planning, and what does the skeleton ship?](11-planning-hooks.md): the "minimal planning pass" is fixed. Ship `Post.author` writing `PostsHint { with_author }` accepted by both `Query.posts` and `User.posts`, plus one hint written from under the interface position, proving reach through an abstract arm; assert via a fetch log on the app context that the accepting resolver saw both, and run the pass in the reference executor for the equivalence test. `Context<C>` is the per-scope view; Plan entries exist for every (node, partition leaf) before generation 0.

From [How does SDL become generated code, and how does the runtime get the schema?](12-build-pipeline-and-crate-layout.md): the skeleton's crate layout is `greem`, `greem-build`, `greem-macros`, `greem-reference` (unpublished) and `examples/axum`; `build.rs` is `greem_build::compile("schema.graphql")` and the app does `mod schema { greem::include_schema!(); }`; the SDL declares one custom scalar served by `Codec::Uuid` (or a hand-written `impl greem::Scalar for types::…`) and the axum handler goes through `greem::http::Request`, `Schema::parse`, `execute` and the multipart part encoder with `Accept` negotiation in the handler. `build()` performs the greem-build version check. Workspace `rust-version` follows stable minus two (1.96 today), edition 2024, and CI builds that floor. The spec document that resolves this ticket points at the generated `OUT_DIR/greem.rs` as the readable reference for what codegen emits.

From [How is the BFS executor shown spec-correct?](13-compliance-harness.md): the "equivalence test against the reference executor" is the `greem-compliance` crate: `greem-reference` producing `serde_json::Value` directly, the property schema with generated document, world and interleaving under `proptest` and `futures::executor::block_on`, the equivalence, call-count, determinism and fold properties, and a small hand-written set including the three cancellation shapes. The graphql-js port onto area schemas and `greem-bench` are follow-ups after the map, not skeleton work. The compliance `build.rs` uses `file_name` per schema and `reference_executor(true)`.

From [Abstract-type derive over a user enum](15-abstract-type-derive.md): the skeleton's interface position uses `Either`/`As`; `#[derive(greem::Abstract)]` is fully designed there and is implemented after the skeleton, not as part of it.

From [What execution-depth limits bound retained generation frames?](17-generation-depth-limits.md): the executor is one barrier over several chains (stream driver per `Streamed` output, mutation driver at the root, parked and retired per the ticket), and the result storage for a chain's slots is owned by the chain, not a request-wide `'req` arena, so ticket 08's `Value<'req>` must be reconciled with per-chain ownership. Tree build counts execution depth with the ticket's rules (`@defer` and `@stream` add one each; lists, fragments, aliases, partition and introspection add none; mutations take the max over roots), fails fast during descent and rejects on the request-error path. `Schema` builder gains `max_depth` (default 32), separate from apollo's parser and introspection limits. `greem-compliance` ships the depth knob property, the at-limit and 2×-default tests on a 2 MiB stack in debug (completion, cancellation, panic), the 1,000-turn stream and many-root mutation depth-independence tests, and runs the reference executor on a larger explicit stack.

From [Does a Streamed output survive retained generation frames?](18-streamed-output-prototype.md): retain the structured borrowed `Payload` sink with inspectable owner/dependent frames (`self_cell` behind the private runtime surface). This supersedes the literal opaque-async-frame / `FuturesUnordered<BoxFuture>` storage sketch, not the barrier or sibling-chain semantics. Internal completion consumes resolver outputs to move streams into owned continuations; object columns then borrow their child sets. Preserve the item-outlives relationship of the proved `Streamed<S>` wrapper. Release incremental work after its parent payload actually ships, not after an empty barrier; stop failed-group descendants while keeping unrelated groups alive; retire only after the last payload reads the storage. Use the integrated and depth probes as implementation references, but implement the real arena, null pass, wire payloads, cardinality checks, tree depth rejection and codegen rather than treating the trace harness as production code.

## Answer

Resolved 2026-09-28. **The skeleton runs end-to-end**, in-repo, from SDL through
`greem-build` codegen to the breadth-first executor, with one `@defer` and one
`@stream` query delivered incrementally over `multipart/mixed` from the axum
example. The spec that describes the core and points at the code is
[docs/spec.md](../../../docs/spec.md); the readable reference for what codegen
emits is the generated module itself (`target/debug/build/greem-compliance-*/out/property.rs`,
about 3,000 lines for the property schema).

### What ships

- `greem` (runtime, ~8k lines): the per-field `Resolver` contract from ticket 07
  (split lifetimes, `parent_error`, `hints`, `plan`), the `Outputs` → sealed
  `Completes` bridge, `As`/`Either`, `Streamed`, `Context<C>` views, the
  execution tree with `CollectFields` and defer/stream usages, the frozen plan
  table (two arrays, one walk, hints per accepting field), inspectable
  `self_cell` frames, the generation loop with one barrier, columnar per-scope
  result storage, the subtree-rooted null pass with `Propagated` slots, all
  three `ErrorBehavior` modes, serial mutation roots, introspection through
  apollo, delivery groups with release/announce/turn/retire, the borrowed
  `Payload` sink, `greem::http`.
- `greem-build`: `compile`/`configure` with `file_name`, `scalar` codecs
  (`Uuid`, `Json`, `String`, `I64`), `absent_aware`, hidden `reference_executor`;
  quote + prettyplease output; `@defer`/`@stream` definitions supplied when the
  SDL lacks them.
- `greem-macros`: `#[greem::object]` with the ticket-05 tail rules, name
  override, hints/plan routing. The `Abstract` derive is a stub (ticket 15
  placed it after the skeleton).
- `greem-reference`: the depth-first oracle, sharing only the tree and the plan walk.
- `greem-compliance`: property schema with a sub-interface, union, enum, custom
  scalars, `absent_aware` input, mutations, streamed list; generators for
  documents, worlds and interleavings; the seven properties (equivalence, HALT,
  incremental fold, call count, determinism ×2, depth limit); 22 hand-written
  spec cases; six depth/cancellation/panic/1,000-turn/1,000-root evidence tests
  on a 2 MiB debug stack.
- `examples/axum`: schema, resolvers in both styles, a lookbehind hint, JSON
  and multipart negotiation on `Accept`, four router tests.

Evidence: `cargo test --workspace` is green (75 tests);
`PROPTEST_CASES=5000 cargo test -p greem-compliance --test properties` passes.

### What the build changed or found

- Ten executor bugs were found by the properties, all in incremental delivery
  and mutation edges (errors lost beneath nulled objects in serial roots;
  premature announcement of nested groups; group order depending on allocation
  order; child fields grouped without the ancestor rule; turn retirement
  dropping deferred work; stream items tagged with the wrong group; a deferred
  fragment whose only fields live in stream items completing early; a merged
  field with differing `@stream` directives; a live-lock in serial mutations
  under incremental delivery; quadratic re-polling of parked roots). Each is a
  regression case now.
- Contract deviations are listed in the spec: `Context<'req, C>` borrowing the app value and plan table (chosen over `Arc`s in a follow-up on 2026-09-29),
  `Completes::reference` in place of the two reference support traits, stored
  object paths, column-local error ids, scope quiescence, the merged-stream
  validation, no debug assertion on cardinality.
- Apollo's parser handles 100 nested levels at its default limit; the only
  depth cap met in testing was serde_json's 128-level parse limit in the tests
  themselves (`unbounded_depth`).

### Review follow-ups (2026-09-29)

Two review rounds after resolution changed: `Context<'req, C>` (borrowed, no
`Arc`s); clippy-clean workspace; nullable stream item errors keep the source
alive; a parent's stream items ship once nothing beneath them is live for that
parent's group, so nested streams are released rather than waited for; retired
turns free their slots and values; each nested `@defer` level counts toward
the depth limit; `@defer`/`@stream` definitions are supplied independently;
recursive input objects are boxed; incremental entries within a payload are
ordered parents-first; fragments nested in fragments but delivered in stream
items carry an `after` dependency on the enclosing group; the group-reading
hint case documents the fold property's third precondition. A third round:
retired turn slots are reused and freed; `ErrorBehavior::Null` keeps a
non-null item's source alive; every selection, `__typename` included, counts
toward the depth limit; supplied directives are detected from parsed
definitions; a nulled parent's groups are dropped at announcement so sibling
streams release; and a subtree stays non-quiescent until its deferred groups
ship. A fourth round: error records live in the turn that owns their slots
and retire with it; delivery groups are reference-counted and reclaimed by a
sweep at each barrier; a turn made in a reused slot counts as progress; groups
abandoned under a failed ancestor are reclaimed too. A fifth round: the
hand-written schema fixture is shared by three test crates; one-use wrappers
inlined; an initial-pull error at a non-null item stops pulling; non-finite
floats are execution errors; argument structs and Plan items nest per type and
`Schema` joins the mangled names; errors order by generation, scope, field,
object; deferred roots are collected per scope; the name mapping is injective
and mirrored in the macro; generated code fully qualifies std names. A sixth
round: the name mapping moves to `greem-core`, shared by codegen and the macro;
the macro strips `r#` before deriving field names; a stream source ending
counts as progress so its group completes on its own barrier; enum helpers are
`greem::Enum` trait items so any value name compiles; objects are `Send + Sync`
by the trait and the spec says so. A seventh round: custom scalars reuse the
leaf completion emitter; the application fixture (types, resolvers, schema
constructor) is shared by the runtime and reference tests; `Halt` wakes the
loop and ships the halted group past pending siblings, and dead scopes go
quiescent; `@defer`/`@stream` arguments follow argument coercion (unprovided
variables take the definition default, explicit null on `if` is a request
error); `Scalar::Value` is `Clone + Debug`. An eighth round: the error that
halts a group is kept at record time and shipped even from a pending column;
lazy pulls stop at the first error under `Halt`; halted stream groups fail
with that error before readiness and death checks; directive defaults come
from the schema's definitions. A ninth round: an initial-pull error under
`Halt` is handed to the barrier before the next parent's pull; `label`
defaults come from the schema too. A tenth round: a hint write goes to the
nearest accepting field above its writer; a failed root is reported once at
the empty path (`Completes::parent_error`, `Outputs::__parent_error`);
infallible per-object macro methods keep their output type, so slices and
`Streamed` complete; `Halt` stops every pull at the first error item and
fails halted stream groups before any turn exists; the initial pull skips
parents whose group already died. An eleventh round: fragment spreads are
collected once per node and delivery context across merged field occurrences
(deferred spreads stay separate usages); generated code names `str` by full
path; macro impls carry the method's `#[cfg]` and `#[cfg_attr]`; `build()` requires both roots
for what the schema declares. A twelfth round: releasing a group counts as
loop progress, so chains of field-less deferred fragments reach their
descendants instead of ending early; generated `hints`/`plan` hooks carry
their hook method's `#[cfg]`/`#[cfg_attr]`. A thirteenth round: the generated
module embeds greem-build's version as a literal, so the skew check at
`build()` compares generator against runtime. A fourteenth round: a parent's
streamed items ship in list order across turns; `Value::UInt` and
`InputValue::UInt` keep integers above `i64::MAX` exact. The property
generator now varies stream capacity and streams root lists; that surfaced and
fixed three more delivery bugs (a lazily drained list completing its parents'
stream group, groups under a not-yet-shipped item dropped as nulled, a nested
stream retired with its parent item's turn). A fifteenth round: `Halt` stops
a pull at an item whose completion would fail (`Completes::first_error`,
`ToLeaf::leaf_error`), not only at a source error; null at a non-null list is
an input error instead of a one-item list; serial mutation roots order their
errors by root field, subtree included. A sixteenth round: `Groups::release`
became `release_ref`; two one-use helpers inlined; a defer nested in merged
stream items depends on the fragment its own occurrence sits under, is never
announced once that fragment failed, and completes with its error if it fails
later; arguments coerce against the concrete object's field definitions;
`Disabled` no longer validates `@stream` arguments. A seventeenth round:
merged `@stream` directives compare resolved arguments, not syntax; fragment
visitation is keyed by the occurrence's enclosing usage too. An eighteenth
round: a field set shared by several fragments runs under its own wire-less
group instead of the last fragment's, so one fragment failing no longer
discards the others' data. A nineteenth round: a nested fragment never
completes before its enclosing fragment, checked at the barrier rather than
only when its scope starts, and for failed or halted completion as well as
successful; a shared field set's record is kept until its members have
completed. A twentieth round: a stream pull also stops under `Propagate` at
an item whose completion error its non-null position cannot absorb
(`first_error` takes a predicate over the error's position). A twenty-first
round: a stream group is dropped at announcement when its list position was
nulled; a leaf completing to null at a non-null position is an execution
error; the macro writes `::core::marker::Send`. A twenty-second round: the
stream probe also sees a leaf that completes to null at a non-null position
(`ToLeaf::leaf_is_null`, `Completes::null_leaf`). A twenty-third round:
custom-scalar variables run their codec during preparation
(`SchemaInfo::check_scalar`), so an invalid one is a request error. Spec
updated.

### Handed to the map

Polling efficiency (every poll traverses the non-quiescent tree; a ready queue
is the next step) joins the fog. The graphql-js fixture port, `greem-bench`,
the `Abstract` derive, SSE and the `onError` wire attribute stay as recorded.
