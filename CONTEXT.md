# greem

An async Rust GraphQL server framework: schema-first codegen and a breadth-first executor that resolves each field once across the objects of a scope instead of once per object.

## Language

### Execution

**Generation**:
One round of the executor: every field of every scope at the current level is resolved once, and the results form the next level's scopes. Partition happens inside the round, so an abstract position never costs an extra generation.
_Avoid_: Level, layer, round, depth

**Execution tree**:
The data-free shape of one request: one node per response key with its merged selections and `@skip`/`@include` already applied. Built before any resolver runs; scopes hang off its nodes.
_Avoid_: Plan (that is lookbehind planning's output), query AST, document

**Scope**:
The objects belonging to one parent scope, field tree position and partition branch, resolved together within a generation. A scope has one concrete GraphQL type and Rust type; equal types in different branches need not share a scope.
_Avoid_: Subtree, node, frame

**Object storage**:
Resolver outputs retained while dependent scopes and response data borrow them: object batches in frames, and owned list outputs kept by a frame or stream turn so their items complete by reference.
_Avoid_: Object arena (the superseded shared-arena design), heap, cache, pool

**Generation frame**:
The owner of one generation's completed output batches, kept alive while descendant generations borrow them; frames nest, one per generation of a chain.
_Avoid_: Arena, stack frame, level

**Chain**:
A nested run of generation frames rooted where its objects were produced (the request root, a stream turn, a mutation root field), stepped in lockstep with every other chain at the barrier. A chain parks when its work is done and retires when its storage drops.
_Avoid_: Task, subexecutor, branch

**Park**:
What a chain does once all work beneath it is complete: it stops advancing but keeps its frames alive until it retires.
_Avoid_: Finish, complete, block

**Retire**:
Dropping a parked chain's storage, at the first barrier after the last payload that reads its data has shipped. Not "release", which is what happens to a delivery group.
_Avoid_: Release, unwind, drop, free

**Signal**:
The flags of one scope, raised on it and every ancestor by its futures, sources and the barrier, so polls and barrier passes descend only where something changed. The root's signal is the request's waker.
_Avoid_: Ready queue, dirty bit, wake list

**Hold**:
One unit of unfinished work that delivers under a delivery group: an object of a working scope, a waiting deferred set, a live stream parent. A group is examined for completion when its last hold goes.
_Avoid_: Reference (that is `refs`, which keeps the group's slot), liveness flag, pin

**Execution depth**:
The number of nested generation frames an operation can require, fixed at tree build: one per nested composite selection, one per deferred fragment, one per streamed list; lists, fragments, aliases, partition and introspection add none, and mutation root fields take the maximum, not the sum.
_Avoid_: Query depth, nesting, generation count

**Result arena**:
The index-aligned record of resolved values, errors and list shapes, laid out per scope and per field and retained by the chains that own its data. Post-hoc null propagation settles a delivery group's values before its payload is serialized.
_Avoid_: Response tree, JSON tree, output map

**Slot**:
One position in the result arena: a value, an intentional null, an execution error raised at that position, a propagated null, or a link to a child scope's objects.
_Avoid_: Cell, entry, result

**Execution error**:
An error raised at one response position by a resolver, an object failure or the framework; the only way a non-null position ends up null, since the type encoding forbids returning null there.
_Avoid_: Field error, resolver error, null violation

**Propagated null**:
A slot rewritten to null by post-hoc null propagation because an execution error beneath it could not be absorbed; distinct from an intentional null and remembers the error that caused it.
_Avoid_: Bubbled null, nulled ancestor

**Error behavior**:
The per-request choice of what an execution error does to the data: null the position, propagate to the nearest nullable ancestor, or halt the operation with a single error.
_Avoid_: onError mode, error mode, error policy

**Set-based resolver**:
The executor's only resolution primitive: a field's resolver is called once per scope with the whole object set and returns one output per object.
_Avoid_: Batch resolver, bulk resolver, dataloader

**Per-object resolver**:
Sugar over a set-based resolver: the user writes the one-object case and codegen supplies the set-based form.

**Post-hoc null propagation**:
Execution errors are not bubbled during execution; a pass over a completed subtree walks up from each error and turns the nearest nullable ancestor into a propagated null.
_Avoid_: Bubbling (during execution), error unwinding

**Partition**:
Splitting abstract outputs into concrete member sets according to their representation branches, which may keep repeated types separate. Partition determines child scopes and the concrete type used for `__typename`.
_Avoid_: Type resolution, resolveType, dispatch

**Lookbehind planning**:
A bottom-up pass before execution in which a field leaves hints its ancestors act on, replacing lookahead (which cannot see through unresolved abstract types). Runs over every concrete arm a resolver can return, so an abstract position does not stop it.
_Avoid_: Lookahead, query planning, preloading

**Plan entry**:
The per-request record for one execution-tree node and partition leaf: the selected fields by delivery group, their converted arguments, and the hint slots of each accepting field. Shared by every scope produced at that position.
_Avoid_: Plan cache (nothing is cached), node data, side table

**Plan table**:
The per-request set of Plan entries, filled during tree build and lookbehind planning and frozen before the first generation runs.
_Avoid_: Plan store, plan arena, cache

**Hint**:
A typed note a field leaves for an ancestor during lookbehind planning, describing what it will need; it is part of the ancestor's meaning, so a field may answer differently depending on what its descendants asked for.
_Avoid_: Attribute, planning note, lookahead, fetch hint (implies it cannot change the result)

**Accepting field**:
The ancestor field whose resolver declared a hint type; a hint travels to the nearest accepting field above its writer and is handed to that field's resolver at execution time.
_Avoid_: Planning root, target field, hint owner

**Delivery group**:
One unit of incremental delivery the client sees as pending and later completed: a deferred fragment at one concrete object, or a streamed list at one concrete parent. The initial group is everything in the first payload.
_Avoid_: Deferred fragment (as the execution unit), payload, incremental result, work-queue task

**Shared field set**:
The fields of one object that several of its delivery groups select. It runs once, ships with the first of those groups to complete, and fails all of them only if it fails itself; the client never sees it as a unit of its own.
_Avoid_: Shared group (it is not a delivery group), merged fragment

**Delivery boundary**:
The response position at the root of a delivery group; post-hoc null propagation that would cross it fails the group instead of rewriting what was already delivered.
_Avoid_: Fragment root, payload root

**Announcement**:
The `pending` entry that tells the client a delivery group exists, sent in its parent's payload; the group is released once that payload has shipped.
_Avoid_: Registration, pending (as a verb)

**Release**:
The moment a delivery group's parent payload has shipped and its scopes may be enqueued; a group whose parent position was nulled is dropped instead.
_Avoid_: Trigger, schedule, kick-off

**Streamed list**:
A list output a resolver produces lazily; drained in place when the request does not stream that field.
_Avoid_: Async list, iterator output, lazy Vec

**Stream turn**:
One batch of streamed items, from any parent of the scope, completed together in one generation and delivered as one incremental list result per parent.
_Avoid_: Chunk, page, tick

### Codegen

**Schema compilation**:
The build-time step that turns SDL into the generated schema module; runs from the user's build script before the crate compiles.
_Avoid_: Schema build, code generation (as the step's name), proto step

**Generated schema module**:
The Rust module the user includes from schema compilation's output: type tags, field markers, input types, the schema builder, and the executor-facing items behind them.
_Avoid_: Generated code (as a noun for the module), bindings, stubs

**Field resolver impl**:
The codegen primitive: one trait implementation per schema field, on the user's Rust type for that object type. Distributed across files and blocks freely.
_Avoid_: Object impl (as the primitive), service impl

**Field marker**:
A generated zero-sized type that names one schema field; a field resolver impl is keyed by it, and the field's argument type hangs off it.
_Avoid_: Field type, field token

**Schema boundary**:
The point where the user supplies root Rust types and the generated schema requires their full resolver contract. Missing or mistyped resolvers may also be rejected at referring output definitions.
_Avoid_: Registration, schema build (as the codegen step)

**Request context**:
The per-request handle handed to every resolver; greem owns the handle and its executor-facing parts, the application owns the typed value inside it.
_Avoid_: Data bag, extensions, global state

**Root value**:
The user's object for the root operation type, supplied per request; every other Rust type in the graph is inferred from resolver outputs starting here.
_Avoid_: Root resolver, context object

**Outputs**:
The user-facing bound stating that a Rust output type can be completed as a given GraphQL type; the only thing a resolver's output must satisfy.
_Avoid_: Shape, completable, type mapping

**Completes**:
The executor-facing counterpart of Outputs, stated from the GraphQL type's side: a generated type tag can complete values of a Rust type. Every generated completion impl is a Completes impl; Outputs is the single bridge over it.
_Avoid_: Complete (verb-form for the trait), resolver bound

**Member wrapper**:
The marker a resolver wraps a concrete value in to return it at an abstract-typed position ("this value, as User"); heterogeneous outputs compose several of them. Scales with what is returned, not with how many types implement the interface.
_Avoid_: Variant, type mapping, union enum

**Member enum**:
A user enum, one member value per variant, that outputs an abstract type; derived sugar over member wrappers in which every variant stays its own partition leaf.
_Avoid_: Union enum, variant mapping, type mapping

**Reference executor**:
A naive depth-first executor kept only to prove the breadth-first executor equivalent under property tests; it builds the response directly and bubbles nulls the spec's way, sharing nothing with the breadth-first machinery but the execution tree and lookbehind planning.

### Compliance

**World**:
The in-memory dataset compliance resolvers read, together with its harness: seeded by counts and generated per property case alongside the document and the interleaving, or written out literally per ported case. The property world is immutable, so its resolvers are pure functions of it; a ported world may hold the state its suite's mutations change.
_Avoid_: Fixture data, mock, test database, root value

**Harness**:
The executor-facing controls every compliance world embeds: the failure map, the interleaving's yield counts, the call log and the gate.
_Avoid_: Test context, controls, rig

**Interleaving**:
The order in which concurrently running resolvers complete, made a generated input by assigning each resolver call a yield count.
_Avoid_: Schedule, timing, race

**Area schema**:
One compliance schema per upstream graphql-js execution test suite, the superset of every schema shape that suite's cases build, onto which those cases are ported one-to-one; extended before another is added. Distinct from the property schema the generators walk.
_Avoid_: Test schema, fixture schema, spec-section schema

**Workload**:
A portable benchmark case: schema, document, dataset description and resolver contract, consumable by any framework in any language.
_Avoid_: Bench, scenario, benchmark (as the case)
