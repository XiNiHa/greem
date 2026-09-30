# Does the Outputs<Ty> trait encoding survive breadth-first execution?

Type: prototype
Status: resolved
Assignee: Iha Shin
Blocked by: 05, 06
Map: ../map.md

## Question

Prototype the type-level encoding: a trait (provisionally Outputs<Ty>) that says a Rust output type is completable as a given GraphQL type, working over sets of parents rather than one value. Stress: nested lists, nullable inside list, abstract types (per 06), mutually recursive object types, and a field whose output type is a generic. Measure compile time and error readability for a mis-typed resolver. Outcome: does the bet hold (per-field `Resolver<F>` impls plus generated per-type `Completes` impls, so a missing or mistyped resolver is a compile error at the schema boundary), and what changes to 05/06 does it force?

Additional stress points from ticket 05's resolution: (1) `type Output<'a>: Outputs<F::Type, C> where Self: 'a` with outputs borrowing from `parents: &'a [&'a Self]` — confirm the associated-type bound compiles for mutually recursive object types without an overflow/cycle error, and what the error reads like when one field impl is missing; (2) the request-lifetime arena: scope objects stored as references into an append-only arena that later generations extend while earlier references are live, with type erasure through a greem `dyn` trait bounded by the request lifetime (no `Any`), and all field futures of a generation joined while borrowing it; (3) `C` threaded through `Resolver`/`ObjectResolver`/`Outputs`, including one impl generic in `C` next to concrete ones; (4) the `ObjectResolver` blanket impl coexisting with hand-written set-based impls under coherence.

From ticket 06: the encoding under test is the bridge design (generated `Completes<T, C> for types::X` with the tag as `Self`; one greem blanket `Outputs<Ty, C> for T where Ty: Completes<T, C>`), not a blanket on `T`. Stress specifically: (5) the `Resolver`-level delegation impls for `&T` and `Result<T, Error>` (the latter bound-only with an unreachable body): does a cleaner shape exist that keeps `Output = Result<T, Error>` and `Output = &'a T` without a "for any tag" blanket in greem; (6) abstract positions via `As<Tag, T>` and per-abstract-type `Either<A, B>` impls, including sub-interface delegation and the partition method's lifetime signature over `&'req [&'req Self]`; (7) that a derive in the user's crate can implement `Completes<UserEnum, C>` for the `include!`d tag.

From ticket 08: (8) object arena as a bump allocator behind std `Allocator` on Rust 1.100 (beta until release): `Box<[Output], &'req Bump>` owned by the scope for Drop while child scopes hold `&'req Output` into it — find a safe shape or the minimal unsafe; (9) the executor seam `Scope::run(&'req self, …) -> BoxFuture<'req, Vec<Box<dyn Scope<'req, C>>>>` with fields joined statically inside generated code — confirm `&'req self` survives the join and that `dyn Scope<'req, C>` needs no `'static`; (10) the per-(node, object type) `types::User::Plan` cache erased as `Box<dyn Any + Send + Sync>` and downcast by generated code, with `Args<F>` borrowed `&'req` from it.

## Comments

### Prototype session — 2026-09-26 (decision pending)

The [compiler-probe report](../prototypes/outputs/README.md) records executable inputs, diagnostics, timing, and explicit limits. The bridge and recursive type bounds compile across a crate boundary. The blanket per-object adapter conflicts with reference/Result delegation and with a context-generic manual resolver. An explicit generated per-field adapter compiles. Splitting the parent-slice borrow from the object lifetime fixes a concrete reference-forwarding failure. The original owning generation loop and ordinary mutable Plan cache also have borrow failures; independent arena probes investigate alternatives.

First decision put to the human: should per-object sugar generate explicit per-field `Resolver` impls instead of the global `ObjectResolver → Resolver` blanket? Recommendation: yes; preserve macro-free use through explicit impls, with an explicit adapter macro as a possible convenience.

This is a provisional prototype, not a resolution. Full executable completion (especially Result normalization), partition, and request-arena integration remain to be proved after discussing the necessary API changes. Existing decisions and glossary have not been rewritten.

Prototype context pointer: local branch `codex/prototype/outputs-encoding`, commit `3e18c577d7c8850f8258995947ee65772dec2fec`. It captures the compiler probes and [arena ownership experiments](../prototypes/outputs/arena/README.md), including diagnostics and limits, without changing the main checkout or its staging area. The snapshot is provisional; it records no adopted resolution.

### Q1 — agreed with Iha Shin

Per-object sugar generates explicit `Resolver` implementations for each field. Remove the global `ObjectResolver → Resolver` blanket. Distributed field implementations and hand-written set-based resolvers remain supported; macro-free users can write the bridge explicitly. This settles adapter placement; whether a separate convenience adapter macro is worth exposing can be handled with the macro surface. The rest of this prototype ticket remains open.

### Next frontier after Q1

The [executable Result completion probe](../prototypes/outputs/completion/README.md) runs across a runtime/application crate boundary. It keeps `Output = Result<T, Error>` through a proposed default `Resolver::parent_error` hook, called by generated object completion through a fixed field witness before child scheduling. Successful positions resolve together; nested errors and borrowed outputs work; no delegation body is unreachable. This mechanism is **not yet adopted**.

The [ownership follow-up](../prototypes/outputs/arena/ownership-followup.md) adds a successful cancellation case for safe nested generation frames. A flat request-wide arena is still unresolved: reverse destruction order alone is insufficient for a general safe arena accepting arbitrary borrowed values with Drop. The safe frame alternative changes lifetime/response ownership; a borrowing response cannot escape unwound frames.

Pending human choices: Q2, adopt the completion hook or investigate another error representation; Q3, prototype integrated nested generation frames or continue investigating the flat arena. Recommendation: adopt the hook and explore nested frames, preserving breadth-first scheduling and arbitrary ordinary Drop behavior. These choices are directions for the remaining proof, not ticket resolution.

Updated prototype context pointer: `codex/prototype/outputs-encoding` at `2f0bbfc508bb35117107ec4d8cc9fc50ae4e36a5` captures the Q1 agreement, executable completion candidate, and cancellation/ownership follow-up. Q2 and Q3 remain pending.

### Q2 and Q3 — agreed with Iha Shin

Keep `Output = Result<T, Error>` with the reserved default `Resolver::parent_error` hook. Normal user implementations inherit None; wrapper delegations forward the error. Generated object completion checks it through a fixed field witness before creating scopes, independently of selected fields.

Use safe nested generation frames for the integrated prototype. Scheduling remains breadth-first with generation barriers; ancestor storage stays alive until descendants finish. The next probe must make response serialization while borrowed owners are retained concrete. The unrestricted flat request-lifetime arena is no longer the direction being prototyped.

### Integrated probe — remaining contract choices

The [integrated prototype](../prototypes/outputs/integrated/README.md) now runs the completion bridge, nested list/null/error handling, sub-interface partition, recursive User/Post resolver requirements, explicit per-field sugar, generated static field joins, scope joins, Plan cache borrows, borrowed serialization and cancellation cleanup together. It uses safe nested generation frames and no object Any or unsafe. Warm run/build: approximately 0.55s; intentionally missing/mistyped resolver builds fail in approximately 0.16s/0.12s. These are tiny-fixture measurements, not compile-scaling claims.

It exposes two remaining API decisions: Q4, include partition-arm identity in a scope and permit repeated same-type arms to resolve separately; Q5, expose borrowed serialization through a callback primitive plus an owned serialized-output convenience. Both are recommended and awaiting the live discussion. The serialized artifact is a completion trace, not the production result arena/GraphQL payload; the linked README states the integration limits.

### Q4 and Q5 — agreed with Iha Shin

Allow separate scopes for nonempty abstract partition arms, including repeated arms with the same GraphQL tag and Rust type. The guarantee is one resolver call per nonempty scope; callers may explicitly normalize repeated arms when they need a single batch.

Use a callback primitive for the borrowed response, plus a convenience API returning owned serialized output. Serialize once while the retained generation frames are alive, then drop those frames. Do not require copying borrowed leaf strings into an owned response arena first.

## Answer

**The encoding survives breadth-first execution with the revisions below.** The [integrated prototype](../prototypes/outputs/integrated/README.md) compiles and runs across a runtime/application crate boundary, with real completion, partition and delegation methods, joined Send futures, request/parent-borrowed data, serialization and cancellation. Iha Shin accepted the five resulting contract choices in the live discussion above. This resolves the type/lifetime bet; the walking skeleton remains a separate ticket.

### Resolver and completion contract

1. Keep the single `Outputs<Ty, C>` bridge over generated `Completes<T, C> for types::X` impls. There is no user-maintained type mapping and no generic completion blanket over every tag. Mutually recursive object requirements compile with the bound on `Resolver::Output`, including nested lists, nullable positions, generic outputs and mixed generic/concrete context impls.
2. Per-object sugar emits an explicit `Resolver` impl per field. Remove the global `ObjectResolver → Resolver` blanket, which overlaps wrapper delegation and context-generic manual impls. The primitive and distributed implementations remain available without the object macro; an explicit bridge is the macro-free route.
3. Split the temporary parent-slice lifetime from the parent-object lifetime. The revised primitive is:

```rust
trait Resolver<F: Field, C = ()>: Send + Sync {
    type Output<'obj>: Outputs<F::Type, C> + Send
    where Self: 'obj, C: 'obj;

    fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<F>,
        ctx: &'obj Context<C>,
    ) -> impl Future<Output = Result<Vec<Self::Output<'obj>>, Error>> + Send + 'call
    where 'obj: 'call;

    // Reserved framework hook; normal user impls inherit this default.
    fn parent_error(&self) -> Option<&Error> { None }
}
```

`'obj` is the lifetime of the retained ancestor storage available to this invocation, not a promise of one universal borrow lasting through the entire request. Temporary slices can be built during wrapper delegation and dropped after the resolver completes while its outputs continue borrowing objects. The required `C: 'obj` GAT bound does not make context or objects static. Generated field markers are static; application data need not be.

4. Keep `Output = Result<T, Error>` and borrowed object outputs. Reference delegation forwards the reserved hook; Result delegation returns its Err or forwards through Ok. Generated object completion selects one fixed field witness and checks that hook before creating any child scopes, independently of requested fields, including a __typename-only selection. Normal field implementations do not override it for field-specific validation. A failed object is recorded once at its output position and never reaches child resolution. Real delegation resolves the successful parents; erroneous direct invocation returns an outer error instead of an unreachable body. Scalar/list/nullable runtime tags and generated abstract tags provide their own disjoint fallible completion impls.
5. Retain `Output: Send`; do not add a blanket Sync requirement. Owned output batches can remain in frames without sharing references to their entire values. The objects projected into concurrently executing scopes are Sync through their Resolver bounds. A separate probe retains a Send, non-Sync output while borrowing only a Sync projection; sharing the entire non-Sync output is correctly rejected.

### Partition and scope identity

Abstract completion is a synchronous typed match through member wrappers, Either and sub-interface delegation. It builds child scopes without adding an execution generation. A user-crate enum may implement completion for its local generated tag; deriving that implementation is still [Abstract-type derive over a user enum](15-abstract-type-derive.md).

Correct the earlier identity guarantee: **a child scope is identified by its parent scope instance, field execution-tree node and generated partition leaf path**. Each leaf has a concrete GraphQL member tag and Rust type, but those properties alone do not uniquely identify the scope. Repeated same-type arms may form separate scopes. Each selected field resolves once across each nonempty scope; there is no automatic cross-arm or cross-parent-scope coalescing guarantee. The integrated fixture deliberately produces User batches of two and one from repeated concrete types. Callers may normalize repeated arms when equality is statically known. The [partition probes](../prototypes/outputs/partition/README.md) explain the generic type-equality/coherence obstacle without claiming impossibility for every conceivable API.

### Ownership, generations and response

Use **safe retained generation frames**, not the literal flat loop borrowing each scope for a universal request lifetime. Each frame owns the completed output batches, projects borrowed objects into typed scopes, joins all current scopes, then awaits the next frame while retaining its own owners. Generated scope code joins its fields statically. Execution remains breadth-first; storage is nested by generation. Object values use no Any, no static bound and no unsafe. Ordinary RAII gives child-before-parent destruction on completion and cancellation in the probes.

The useful separation is owned output batches (Send) from borrowed executable scopes (Send + Sync). Batch completion borrows the retained frame; resolver outputs may borrow its projected ancestor objects. This supersedes the selected `Box<[Output], &Bump>`-owned scope sketch: it had both lifetime/Drop-check problems and a Send problem because Bump is not Sync. Merely hiding a destructor list behind unsafe would not establish a sound arbitrary borrowed-object arena; the [ownership follow-up](../prototypes/outputs/arena/ownership-followup.md) records the constraints and counterexample.

The response primitive is conceptually `execute_with(..., finish: for<'r> FnOnce(Response<'r>) -> R) -> R`, with the relevant Send bounds. Invoke it after the last generation's completion/null pass while every borrowed owner remains alive. `Response<'r>: Serialize` can borrow leaf strings directly. The callback's result cannot borrow data whose validity depends on the retained frames; a convenience API returns owned serialized bytes/text. Serialize once, then unwind/drop the retained storage. This replaces returning a borrowing response after execution's local owners have already been dropped. The callback is synchronous; streaming/network adapters can consume the owned serialized result afterward.

Owned Plan values remain the permitted Any exception. A fixed table of `OnceLock<Box<dyn Any + Send + Sync>>` supports lazy initialization and stable shared argument borrows in the integrated probe. General storage for lazily discovered tree positions needs its own decision, [How are eagerly built Plan entries stored and borrowed per request?](16-lazy-plan-storage.md). Nested polling and destruction have depth-dependent call chains; policy is [What execution-depth limits bound retained generation frames?](17-generation-depth-limits.md).

### Evidence and limits

- Cross-crate compile probes cover recursive bounds, generic outputs/context, wrapper coherence, derive placement, missing-field rejection and a mistyped output. The integrated run adds real methods, nested nullable lists, sub-interface partition, owned and borrowed object outputs, explicit per-field sugar and Plan argument borrows.
- Deliberately pending field futures show both parent scopes start before either completes and child scopes start only after both finish. Completion and cancellation preserve borrowed Post destructor dependencies. Borrowed leaf serialization finishes before their owners drop. A companion compile-fail probe prevents a callback from returning a frame-dependent response borrow.
- Warm local Rust 1.98 measurements for the small integrated fixture: run/build about 0.55s; missing-field and wrong-output failures about 0.16s and 0.12s. These are not cold-build or schema-scaling benchmarks. The allocator-driven MSRV rationale is superseded; [How does SDL become generated code, and how does the runtime get the schema?](12-build-pipeline-and-crate-layout.md) must settle the remaining version policy.
- Missing fields are compile errors, but rustc may lead with the referring Output obligation rather than the schema-boundary call or custom `on_unimplemented` wording. A wrong leaf type is rejected at its associated-type definition. Do not promise a particular diagnostic layout.
- The artifact serializes a path/slot completion trace, not the production columnar result arena or GraphQL response. SDL/codegen/proc-macro implementation, complete field-error/null semantics, lookbehind, mutations, exact result-arena layout and compliance are still their existing tickets. The prototype uses assertions/unwrap for cardinality/invariant failures; it does not implement framework mismatch handling. No production memory-safety claim rests on custom unsafe: the successful ownership path is safe Rust.

Assets are captured on local branch `codex/prototype/outputs-encoding`. The integrated-code snapshot is `9d08172987489484b7b70afd8fbaff5a3596f972`; a later documentation snapshot may record these accepted decisions. Run commands, source and diagnostics are linked from the prototype report.

Final prototype context pointer: `codex/prototype/outputs-encoding` at `9d25d5a1bf1034fccedc297529cd180a89680cc2` includes the executable sources, captured diagnostics and accepted-contract documentation. The main checkout and staging area were not switched or rewritten to capture it.
