# How are interfaces and unions represented on the Rust side without a type mapping?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 05
Map: ../map.md

## Question

The executor must partition an abstract-typed object set by concrete type to build per-type child scopes lazily. Given no user-maintained type mapping, how does codegen know the concrete Rust types behind an interface/union? Candidates: generated generic enum whose variants are inferred through the resolver's bounds; a generated trait with an associated concrete-enum type; something else. Decide, and specify what the user writes to return a heterogeneous list and how the executor dispatches.

## Answer

**Mechanism correction to ticket 05 (found here, verified with a two-crate scratch build; not kept).** The generated blanket `impl<T: Resolver<…>> Outputs<types::User, C> for T` is rejected by rustc (E0210: uncovered `T` before the first local type). The generated type tag must be `Self`. 05's user-facing spelling is unchanged; underneath:

```rust
// greem: executor-facing, tag is Self
pub trait Completes<T, C = ()> { … }
// greem: the only impl of the user-facing trait anywhere
impl<T, Ty: Completes<T, C>, C> Outputs<Ty, C> for T {}
// generated, per object type
impl<T, C> Completes<T, C> for types::User
    where T: Resolver<User::name, C> + Resolver<User::friends, C> + … {}
```

Consequences: greem may not carry any "for any tag" blanket (`Completes<Result<T>> for S`, `&T`, `Either`, `Never`); each conflicts with every generated object impl because a downstream crate could implement `Resolver<marker, TheirCtx>` for the wrapper. `&T` and `Result<T, Error>` outputs work by delegating at the `Resolver` level (`impl Resolver<F, C> for &T` / `for Result<T, Error>`), so one generated impl covers `T`, `&T`, `Result<T>`; the `Result` impl is bound-only with an unreachable body, a smell for ticket 07 to stress. Mutually recursive object types compile through the bridge; a missing resolver reads `no resolver for GraphQL field 'author' with context '()'` at every output type naming the object, as 05 predicted.

**Abstract types: compositional wrappers, generated per (abstract, member) pair.** greem ships `As<Tag, T>` ("T as Tag") and a two-arm `Either<A, B>`. Codegen emits, per interface/union `Node`:

```rust
impl<T: Outputs<types::User, C>, C> Completes<As<types::User, T>, C> for types::Node {}   // one per member
impl<A, B, C> Completes<Either<A, B>, C> for types::Node where types::Node: Completes<A, C> + Completes<B, C> {}
impl<T: Outputs<types::Resource, C>, C> Completes<As<types::Resource, T>, C> for types::Node {} // one per sub-interface in the `implements` chain
```

A resolver returning only users writes `Option<As<types::User, MyUser>>`; a heterogeneous list writes `Vec<Either<As<types::User, MyUser>, As<types::Post, MyPost>>>`. Cost scales with what is returned, never with interface width; no `Never` placeholder exists. Unions and interfaces are identical on the Rust side. Rejected: a generated generic enum with one positional parameter per member (degrades with wide interfaces, shifts on SDL edits); a generated trait with one associated type per member on a user enum (still enumerates every member, hand-written impl on the primitive path).

**Interface fields resolve through the implementer.** No `schema::Node::id` markers; a selection of `id` in a `Node` position desugars in each concrete scope to `schema::User::id`. A shared blanket over `HasId` would itself be an orphan violation, so interface markers would not deliver "implement once" anyway.

**Partition** is a pure, synchronous match on the output value, never async and never user-written; `__typename` and type resolution come from it. The executor partitions while building child scopes, bucketing by member tag *and* Rust type, each object remembering its parent index. Two arms with the same member and different Rust types yield two scopes for that concrete type; scope identity is (Rust type, selection set), so "once per field per generation" is precisely "once per Rust type per generation". Constraint handed to ticket 08.

**Sugar**: a derive on a user enum generating `Completes<MyEnum> for types::SearchResult` is ticketed as [Abstract-type derive over a user enum](15-abstract-type-derive.md), blocked by 07; variant matching (attribute vs name) is decided there.

**Glossary**: Outputs (user-facing) / Completes (executor-facing) / partition recorded in CONTEXT.md.

## Amendment (from the resolved Outputs encoding prototype)

The current contract revisions and their evidence are recorded in [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md#answer). Its resolution supersedes the relevant adapter, error-delegation, lifetime, scope-identity, ownership and response sketches above; the original discussion is retained as history.
