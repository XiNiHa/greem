# What are the execution tree, a scope, a generation, and the result arena?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 
Map: ../map.md

## Question

Define the executor's data model. What a scope is keyed by (concrete type × selection set? per response-key?), how field merging, aliases and per-selection arguments map onto scopes and fields, how lists flatten into the child scope while remembering their parent grouping, how index-aligned objects/results sets are laid out in the arena, how the arena is serialized once at the end, and how a request-level HALT stops the loop. Decide what is fixed at tree-build time vs at execution time, and what the abstract-type lazy build needs from the arena.

Constraints from ticket 05: a scope's objects are `Vec<&'req T>` referencing a per-request arena; resolver outputs may borrow from their parents for `'req`, so owned outputs are moved into the arena and everything the executor stores is bounded by `'req`, never `'static` — scope erasure goes through a greem `dyn` trait, not `Any`. Args are one owned value per selection, shared by all objects of the scope.

From ticket 06: scope identity is (Rust type, selection set), not (GraphQL type, selection set); an abstract-typed output set is partitioned while building child scopes by member tag and Rust type, each object keeping its parent index, and two arms with the same GraphQL member but different Rust types become two scopes. Partition is synchronous and pure, so it needs no generation of its own.

## Answer

Two layers. An **execution tree** is built per request from the validated document: data-free, and variable-free except for `@skip`/`@include`, which are applied at build while the tree records the condition variables it consumed (a later cache keys on document + those values only; fog). **Scopes** hang off tree nodes at execution time.

**Tree.** Nodes in a `Vec` with index children, one node per response key (spec `CollectFields`/`MergeSelectionSets`), field names borrowed from the apollo document, no apollo types past greem's internals. Concrete positions collect their grouped field set at build; abstract positions collect lazily per concrete type that actually appears, cached for the request. Args are not in the tree: a per-request side table indexed by node holds, per GraphQL object type, a codegen-emitted `types::User::Plan` (grouped field set, dense field indices assigned by codegen, converted `Args<F>` per entry), erased as `Box<dyn Any + Send + Sync>` and downcast only by the generated code that built it (owned args are `'static`; the no-`Any` rule applies to the object arena). Dispatch inside a scope is `match field_idx`. Filled once by the first scope of that type at that node; scopes borrow `&'req Args<F>` from it.

**Scope** identity is (tree node, Rust type). No merging across tree positions (cross-position dedupe is ticket 11's lookbehind). A node never creates an empty scope. Objects are `Vec<&'req T>`; each object records its parent scope, parent object index and innermost shape entry. Partition is synchronous inside the parent's completion and yields child scopes directly, so abstract positions cost no extra generation. Root scope objects are `[&root_value]`, root values moved into the object arena first.

**Object arena.** One per request, append-only, `'req`-bounded, Drop guaranteed, no `Any`. Backend: a bump allocator behind std `Allocator` (`Box<[Output], &'req Bump>`, `Vec::new_in`), hence **MSRV 1.100** (allocator API stabilized there; skeleton builds on beta until release). The borrow of `&'req [T]` out of a box the scope still owns for Drop is ticket 07's stress point 8.

**Result arena** is columnar per scope: N objects, one column per field of length N. Leaf column slot: `Leaf(Value<'req>) | Null | Error(id)`. Composite column: flat per-level arrays, one level per `[` in the field type, each entry `Items(range) | Null | Error(id)`, innermost `Object(child_idx) | Null | Error(id)`; the child scope is named once on the column. Links are bidirectional (parent column → child scope + shape; child object → parent position) so serialization walks down, the null pass (09) walks up, and a scope subtree serializes alone (10). `Value<'req>` is `Null | Bool | Int(i64) | Float(f64) | Str(Cow<'req, str>) | List | Object`; built-in scalars and enums write one variant, custom scalars serialize into it via `Outputs`; no `Raw` variant unless a benchmark asks. `__typename` is synthesized at serialization from the scope's tag, never stored.

**Errors** live in one per-request `Vec<ErrorRecord>` (message, extensions, origin scope + object index + field); slots hold the id; the path is reconstructed at serialization by walking parent links and shapes (defer re-roots paths anyway).

**Generation loop.** The executor is a loop over `Vec<Box<dyn Scope<'req, C>>>`: join all scopes of the generation; each `Scope::run(&'req self, tree, arena, ctx) -> BoxFuture<'req, Vec<Box<dyn Scope>>>` is generated code that joins its own fields statically, completes leaves into columns, partitions composites into child scopes and returns them as the next generation. Erasure is per scope, never per field. Mutation: one root scope per top-level field, each chain run to completion serially, arena shared. HALT is a flag observed at the barrier: the current generation finishes, nothing new is enqueued (mid-generation cancellation may hide behind the same flag later, ticket 09).

**Serialization** once: `Response<'req>` is a view over the arena implementing `serde::Serialize` (`data` then `errors`, field order from the grouped field set); greem writes no JSON itself.

**Fixed at build vs execution.** Build: nodes, merged selections, skip/include, consumed condition variables, concrete-position grouped field sets. First scope of a (node, type): grouped field set for abstract positions, field indices, `Args<F>`. Execution: scopes, objects, columns, shapes, errors.

Rejected: JSON-like result tree during execution; recursive per-parent list enum; eager error paths; string-match field dispatch; per-field future erasure; symbolic skip/include in the tree; stable-Rust bumpalo boxes (chosen 1.100 instead).

Assets: [ADR 0001 columnar result arena](../../../docs/adr/0001-columnar-result-arena.md), [ADR 0002 MSRV 1.100 for std Allocator](../../../docs/adr/0002-msrv-1-100-for-std-allocator.md)

## Amendment (from the resolved Outputs encoding prototype)

The current contract revisions and their evidence are recorded in [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md#answer). Its resolution supersedes the relevant adapter, error-delegation, lifetime, scope-identity, ownership and response sketches above; the original discussion is retained as history.
