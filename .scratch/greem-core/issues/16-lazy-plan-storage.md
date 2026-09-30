# How are eagerly built Plan entries stored and borrowed per request?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 07, 08, 11
Map: ../map.md

## Question

Narrowed by [What can a field say to its ancestors during planning, and what does the skeleton ship?](11-planning-hooks.md): nothing is lazily discovered any more. Codegen's plan walk visits every (node, partition leaf) the request can produce at tree build and constructs its Plan entry (grouped field set by delivery group, dense field indices, converted `Args<F>`, hint slots) before generation 0. Confirm the per-request storage for these owned, Any-erased entries (a `Vec` indexed by the walk, or the tree node table itself), the `&'req` borrowing boundary for args and hints across retained generation frames, that partition scopes of the same (node, leaf) share one entry, and how an argument-conversion or hint-registration error at build is surfaced (the whole request fails at build, or the field errors at execution). Object values remain Any-free.

Original question, for history: choose the per-request storage and slot-assignment policy for generated, owned Plan values when abstract scopes reveal new execution-tree positions lazily; a fixed OnceLock table supports stable borrowed Args references, but the integrated Outputs probe preallocates every slot. Context: [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md) and its [integrated probe](../prototypes/outputs/integrated/README.md).

From [How does SDL become generated code, and how does the runtime get the schema?](12-build-pipeline-and-crate-layout.md): argument coercion (`CoerceArgumentValues` over the literal `ast::Value` with variables from apollo's coerced map and schema defaults, then `FromInput`) runs when a Plan entry is constructed at tree build, so an argument failure is known before generation 0; per ticket 05 it is a field error at that selection, not a request failure, which fixes the answer to this ticket's build-vs-execution error question unless you find a reason to overturn it. `Document` is `Arc`-shared and immutable, so nothing in a Plan entry may borrow the document text; hints and args are owned or `&'req` into per-request storage.

## Answer

Agreed with Iha Shin, 2026-09-28. Eleven questions over two rounds; all resolved as recommended.

**Vocabulary** (now in `CONTEXT.md`): a *Plan entry* is the per-request record for one execution-tree node and partition leaf; the *Plan table* is the per-request set of them, frozen after lookbehind planning.

### Storage

1. **A separate per-request Plan table, not the node table.** Owned by the request state beside the execution tree and the error vec, created before generation 0, outliving every retained generation frame. The node table stays data-free; `PlanId` assignment is deterministic per tree, so the tree-cache fog can later cache the tree and rebuild only the entries. Rejected: entries stored on tree nodes (makes the tree per-request); the probe's `OnceLock` slots (nothing is discovered lazily any more).
2. **Two parallel arrays indexed by a dense `PlanId`** (struct-of-arrays). `headers: Vec<PlanHeader>`: node id, parent `(PlanId, field index)`, child links indexed by a codegen-dense (field, partition leaf) index, and one `TypeId`-keyed hint slot vector (`Vec<(TypeId, Box<dyn Any + Send + Sync>)>`) **per selected field**. `typed: Vec<Box<dyn Any + Send + Sync>>`: the generated per-type Plan struct holding field sets by delivery group, dense field indices and `Result<Args<F>, Error>` per selected field. The typed array is written once at construction and never mutated; the header array is the only thing the planning pass touches, so `&typed[e]` and `&mut headers[a]` never conflict. The Any exception stays confined to the typed payload and the hint slots. Rejected: one `Vec<PlanEntry>` with split borrows; a sealed `dyn PlanEntry` trait with an `as_any` hatch.
3. **Identity is (node, partition leaf); one entry is shared by every scope produced there**: all parents in the generation, deferred sibling scopes over the same objects, and stream turns. Repeated arms with the same tag and Rust type are different leaves and therefore different entries; their args are converted twice and `hints` declared twice. Accepted: the cost is proportional to the selection, not the data, and leaf identity is what ticket 07 settled for scopes.

### Walk and freeze

4. **One recursive walk.** On enter: assign the `PlanId`, convert arguments into the typed payload, call `hints` for every selected field. Recurse per field and leaf. On exit from a field's subtree: write the child link into the parent's header and call `plan` for that field (post-order, as ticket 11 requires). Child links therefore live in the header, not the typed payload, which is immutable from construction. Rejected: two walks with `downcast_mut` to backfill child ids.
5. **Freeze after the pass.** No `OnceLock`, no locks, no runtime mutation: the table is shared immutably by every concurrent scope future, by deferred and continuation scopes released in later generations, by all mutation root chains, and by the reference executor, which consumes the same frozen table. A later loader primitive would register during the same pass, before the freeze.

### Borrowing

6. **One checked downcast per scope, at creation.** The parent's completion downcasts the child's typed entry and the generated scope struct holds `&'req TypedPlan`; resolvers receive `&'obj Args<F>` by reborrow, since `'req` outlives any frame's `'obj`. Rejected: downcasting on every `resolve`.
7. **The `Context<C>` view is per field invocation, not per scope.** Because hint slots are per accepting field (two fields of one type may accept the same hint type, `User.posts` and `User.drafts` both taking `PostsHint`), `ctx.hint::<H>()` must know the asking field. The view is a small stack value built inside the scope's `run` future per field invocation: request context, `&'req PlanHeader`, field index. The upward hint walk during planning visits (entry, field) pairs via the header's parent link. This amends ticket 11's "frame-allocated per scope". Rejected: one view per scope searching every field's slots (ambiguous).

### Faults

8. **Argument-conversion failure is a field error at that selection** (ticket 12, spec `CoerceArgumentValues`): stored as `Err` in the entry; at execution the scope writes that execution error at the field slot for every object, never calls the resolver and enqueues no child scope. During the pass the failed field runs neither `hints` nor `plan` and its subtree is not walked. Rejected: failing the whole request at build; walking a dead subtree for speculative hints.
9. **Registry faults.** Duplicate `reg.accept::<H>()` is idempotent with a `debug_assert`. `plan.hint::<H>()` with no acceptor is a no-op (ticket 11). `ctx.hint::<H>() -> &H` panics with a message naming the field and hint type when `H` was not accepted by that field's `hints`; `ctx.try_hint::<H>() -> Option<&H>` serves conditional acceptors.

### Consequences for other tickets

- **Depth limits (17):** the planning walk is recursive over the tree, so its recursion depth equals tree depth and falls under the same bound as the generation frames.
- **Skeleton (14):** ships the two-array table, the single walk, per-invocation views and the field-error path for bad arguments.
- **Planning hooks (11):** view granularity amended per item 7; slots per field rather than per entry.
- **Tree cache (fog):** a cached tree can carry the table size and id assignment; only entries are rebuilt per request.

No ADR: the Any exception and the eager pass are already recorded in tickets 08 and 11, and the remaining choices are cheap to revisit.
