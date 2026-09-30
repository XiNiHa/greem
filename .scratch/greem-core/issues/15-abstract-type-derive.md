# Abstract-type derive over a user enum

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 07, 11
Map: ../map.md

## Question

Design the sugar over ticket 06's primitive: a derive (working name `#[derive(greem::Abstract)]`) on a user enum that generates `Completes<MyEnum, C> for types::SearchResult` by delegating each variant to the member's `Outputs` bound, so a heterogeneous output is `Vec<MyHit>` instead of nested `Either<As<…>, …>`. Decide: how a variant names its member (explicit `#[greem(as = "User")]` attribute vs variant name = GraphQL type name), whether one enum may target several abstract types, whether it lives in greem-macros beside `#[greem::object]`, and how the derive reaches the generated tag from the user's crate (the generated module is `include!`d there, so the tag is local; confirm the impl is accepted). Must be fully opt-out-able: the primitive stays usable without it.

From [What can a field say to its ancestors during planning, and what does the skeleton ship?](11-planning-hooks.md): the derive must expose its variants to codegen's static plan walk the same way `Either`/`As` arms do, so hints written under a derived-enum arm reach ancestors at tree build.

From [How does SDL become generated code, and how does the runtime get the schema?](12-build-pipeline-and-crate-layout.md): the derive lives in `greem-macros` behind the default `macros` feature and takes the same `schema = crate::schema` (default) and `type = "SearchResult"` (default: the enum's identifier) attributes as `#[greem::object]`; the tag it targets is local because the generated schema module is `include!`d in the user's crate, and `Completes` is sealed, so the derive must go through a documented greem entry point rather than implementing the sealed trait directly. Decide that entry point here.

## Answer

Agreed with Iha Shin, 2026-09-28. Eleven questions over two rounds; all resolved as recommended except two where Iha chose differently: the derive is named `Abstract` rather than `Outputs` (Q7), and the reference-executor impl is an explicit opt-in rather than unconditional (Q10).

**Vocabulary** (now in `CONTEXT.md`): a *member enum* is a user enum, one member value per variant, that outputs an abstract type.

### Entry point

1. **The derive emits the sealed `Completes` impl itself, through `greem::__private`**, the same channel the generated schema module uses. `#[derive(greem::Abstract)]` lives in `greem-macros` behind the default `macros` feature (ticket 12) and is the only supported way for an enum to output an abstract type; `Either`/`As` remains the macro-free path; there is no supported hand-written enum route. Rejected: a public `Partition<Ty, C>` trait plus a codegen-emitted per-tag blanket. The [coherence probe](../prototypes/abstract-derive/README.md) shows the blanket overlaps the generated `As` impl (E0119, downstream crates may implement it for `As<User, _>` with their own `C`); dropping `C` makes it coherent but hollow (the blanket claims every context, so a context mismatch is no longer caught); pairing it with a `C`-less tagged marker is coherent (P5) but the public method would have to mirror the sealed `Completes::complete` signature, putting the executor-facing contract into the public API under another name. P1 confirms the direct impl is coherent for a lifetime-and-generic enum and gives the shortest missing-resolver diagnostic.
2. **Membership is checked at the derive site** through a generated per-(abstract, member) witness trait in `__private` on the abstract tag, covering the `implements` chain (so `as = "Resource"` on a `Node` enum is a member whose payload is `Outputs<types::Resource, C>`), with an `on_unimplemented` message naming the non-member. This is what lets a syntactic macro that never sees SDL reject a wrong name at compile time.
3. **Delegation:** each variant is completed through the same hidden per-arm helper the generated `As` impl calls, with the variant index as the partition leaf step; the derive also emits the plan-walk method, recursing into each variant's `Outputs` type in declaration order, so hints written under a variant reach ancestors at tree build (ticket 11).

### Spelling

4. **One `#[greem(...)]` attribute per target**, each with its own `schema` (default `crate::schema`) and `type`; `type` defaults to the enum identifier only when the enum carries at most one attribute. Every variant must be a member of every target; if the sets differ, write two enums. An enum may therefore target abstract types from different generated schema modules in one crate (ticket 13's `file_name` amendment).
5. **Variant naming:** the member name defaults to the variant identifier; `#[greem(as = "User")]` overrides it. Names are exact GraphQL spellings resolved to `schema::types::<Name>`.
6. **Reference executor:** `#[greem(reference_executor)]` on the enum emits the hidden `ReferenceComplete` impl (dispatching each variant to its member) for every target. The `reference-executor` cargo feature on `greem` stays as ticket 12 defined it; using the attribute without the feature fails with a plain missing-trait error naming `ReferenceComplete`. Rejected: making the support traits unconditional; hand-written reference impls in the compliance crate.

### Shape

7. **Single-field tuple variants only**; unit, struct and multi-field variants are macro errors at the variant span. The payload bound is `Outputs<Member, C>`, so `&T` and `Result<T, Error>` payloads work through ticket 07's delegation and `Option`/`Vec` payloads are rejected naturally: lists and nullability wrap the enum from outside. Enum generics and lifetimes are forwarded verbatim (`enum Hit<'a> { User(&'a MyUser), Post(&'a MyPost) }` is the common case). The impl is generic over `C`; there is no `context` attribute because the derive has no bodies that name the context.

### Execution

8. **Variants are partition leaves in declaration order**, for scope order, error order and deterministic ids alike.
9. **Repeated members are allowed** and produce separate scopes (ticket 07's rule); the derive does not normalize because same-looking Rust types are not a type-equality oracle (partition probe). Documented, not rejected, since different payload types for one member are legitimate.

### Placement

10. **Implemented after the walking skeleton**; the skeleton's interface position uses `Either`/`As`. Ticket 14 carries the note.

Assets: [coherence probe](../prototypes/abstract-derive/README.md) (throwaway, P1–P5b on rustc 1.98).
