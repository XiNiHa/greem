# What can a field say to its ancestors during planning, and what does the skeleton ship?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 04, 05, 08
Map: ../map.md

## Question

Define the lookbehind planning pass for greem: when it runs, what a field resolver can register (preloads, hints), how registrations reach ancestor resolvers at execution time, how abstract types participate given lazy scopes, and what the minimal version in the skeleton is. Must fit the per-field trait primitive from 05: resolvers see `ctx: &Context<C>`, which is where planned preloads/slots surface at execution time (greem owns `Context`'s executor-facing methods; the user's value sits behind `ctx.app()`).

From ticket 08: the execution tree is data-free and per request; planning notes therefore attach to tree nodes (or to the per-(node, object type) `Plan` cache entry that codegen emits and that already holds field indices and `Args<F>`), not to scopes. Ancestor mutation during a bottom-up pass is `&mut tree.nodes[parent]` over an index-based `Vec`. Preload results reach resolvers at execution time through `Context<C>` per ticket 05.

## Answer

Agreed with Iha Shin, 2026-09-27. Fourteen questions; all resolved as recommended except three where Iha chose differently: the reference executor runs the planning pass too (Q6), hints may shape a field's result rather than only its fetch (Q11), and hint writers may read their delivery group (Q14).

**Vocabulary** (now in `CONTEXT.md`): a *hint* is a typed note a field leaves for an ancestor during lookbehind planning; the *accepting field* is the ancestor whose resolver declared that hint type; the executor hands the hint to the accepting field's resolver at execution time.

### Scope

1. **v0 planning is hints only.** Cardinal's second primitive, preloads (cross-scope batched loaders run between generations), is the map's "Dataloader / cross-field dedupe" fog and stays there; ticket 08's pointer "cross-position dedupe is ticket 11's lookbehind" now points at that fog entry. The planning pass is the extension point where a loader primitive would register. Rejected: a loader primitive now; preloads without hints.

### When the pass runs

2. **Once per request, at tree build, over the whole tree including every statically known abstract arm.** Codegen walks the Rust type chain from the root value's type through each `Resolver::Output` and each `Either`/`As` arm of an abstract `Outputs` type, so every (node, Rust type) pair the request can produce is visited before generation 0. Unlike Cardinal, greem does not omit abstract positions and re-plan them after partition with sealed ancestors, because the resolver's output type already names the exact arms: a hint written under an interface position reaches ancestors above it. Cost accepted: a hint from an arm that yields zero objects is speculative. The pass is synchronous and sequential, post-order (children before parents, siblings in field order), `&mut` over the node `Vec`; no concurrency story.
3. **Per request, with args and context.** `plan` receives the converted `&Args<F>` and `&Context<C>` (for `ctx.app()`), sync only. The future tree cache (fog) caches the data-free tree and re-runs the pass per request.
4. **Mutations** are planned once for the whole document at build, before the first root chain runs.

### Registration

5. **Two default methods on `Resolver<F, C>`, no associated type** (associated type defaults are unstable on rustc 1.98, E0658):

```rust
trait Resolver<F: Field, C = ()>: Send + Sync {
    // …resolve, parent_error as in ticket 07…
    fn hints(reg: &mut HintRegistry) {}                 // reg.accept::<PostsHint>()
    fn plan(plan: &mut Planning<'_, F, C>) {}
}
```

`hints` runs once per Plan entry at tree build, before the pass, so an accepting field's slot exists before any descendant (which plans first) writes to it. A hint type is `Default + Send + Sync + 'static`. Slots live in the Plan entry as a small `TypeId`-keyed vector of `Box<dyn Any + Send + Sync>`, the permitted Any exception. Generated code calls both methods unconditionally; unplanned fields inherit the no-ops. Rejected: mandatory `type Hint` with `()` boilerplate; a separate opt-in `Planner` trait (uncallable from generated code without specialization).
6. **Addressing is by hint type, nearest accepting ancestor, self included.** `plan.hint::<PostsHint>(|h| h.with_author = true)` walks up from the writer's node and mutates the nearest field whose resolver accepted `PostsHint`; none found is a no-op. `Post.author` need not know whether it sits under `Query.posts` or `User.posts`; both accept `PostsHint`. Because the pass is post-order, an accepting field's own `plan` sees everything its descendants wrote and may forward upward. `Planning` also exposes `args()`, `ctx()` and read-only position facts: field name, parent GraphQL type, and the delivery group. Rejected: addressing by field marker or by GraphQL parent type; copying every hint to every ancestor.
7. **`#[greem::object]` spelling:** optional attributes on receiver-less fns in the impl block, `#[greem(hints = "posts")] fn …(reg: &mut HintRegistry)` and `#[greem(plan = "posts")] fn …(plan: &mut Planning<…>)`, routed into the generated `Resolver` impl for that field. Rejected: a naming convention.

### Delivery

8. **`Context<C>` becomes a per-invocation view**: request context plus this scope's node and Plan entry, frame-allocated per scope (not per object), `ctx.app()` unchanged, plus `ctx.hint::<H>() -> &H` for a type the field accepted. Slots are read-only after the pass. `resolve`'s signature from ticket 07 is unchanged; `Args<F>` stays purely the GraphQL arguments. Rejected: a fourth `resolve` parameter; hints inside `Args<F>`.

### Semantics

9. **Hints may shape results.** A hint is part of a field's semantics, not an advisory fetch strategy: a resolver may legitimately return different data depending on what its descendants asked for. Consequences: the reference executor runs the identical pass (same tree, same post-order) so both executors see identical hints; there is no "defaults only" harness mode; a field's value may depend on which sibling or descendant fields were selected.
10. **Delivery groups are visible to writers.** Hints from deferred and streamed subtrees are applied to their initial ancestors at build; the writer may read its group from `Planning`. Ticket 13's fold-and-compare property is therefore conditional: it holds for documents whose hint writers do not consult the delivery group (the `Disabled` tree has no groups). A per-group split of an accepting field's hint is fog; the group is already on the node, so it needs no data-model change.

### Consequences for other tickets

11. **Plan storage (16).** Eager planning creates every Plan entry at build: the slot space is finite, storage is a per-request `Vec` indexed by the walk, args and hints are borrowed `&'req`, nothing is lazily discovered. Plan identity sharpens from ticket 08's (node, GraphQL type) to (node, partition leaf), since two arms with the same member tag but different Rust types carry different hint declarations. Ticket 16's question is narrowed accordingly; it is not closed here.
12. **Skeleton (14).** One concrete-path hint (`Post.author` writes `PostsHint { with_author }`, accepted by `Query.posts` and `User.posts`) and one hint written from under the interface position, proving reach through an abstract arm. The test asserts through a fetch log on the app context that the accepting resolver saw both, and the equivalence test runs the pass in the reference executor.
13. **Codegen (12)** emits the type-chain walk (a generated per-tag plan method that recurses through `Outputs` arms), calls `hints` at Plan construction and `plan` in post-order, and builds `Context<C>` views per scope. **Compliance (13)** gives the reference executor the same pass and states the conditional fold property. **Abstract-type derive (15)** must expose its variants to the walk like `Either` arms do.

Extension points: loader/preload primitive in the pass (fog: dataloader); per-group hint split (fog); tree cache re-running the pass (fog).

Assets: [ADR 0004 static lookbehind planning at tree build](../../../docs/adr/0004-static-lookbehind-planning-with-semantic-hints.md).

## Amendment (from the resolved Plan storage ticket)

[How are eagerly built Plan entries stored and borrowed per request?](16-lazy-plan-storage.md) narrows item 8: hint slots are per accepting field, and the `Context<C>` view is built per field invocation rather than per scope; child links and slots live in a per-entry header separate from the typed Plan payload.
