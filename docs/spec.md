# greem core: the walking skeleton

Status: the core design is decided and runs end-to-end in this repository.
This document is the map from the decisions to the code; the code is the
source of truth. Read the generated schema module first when you want to see
what codegen emits: build any consumer and open `target/debug/build/<crate>-*/out/*.rs`
(the compliance crate's `property.rs` is the largest example).

## What runs

| Command | Proves |
| --- | --- |
| `cargo test -p greem --test runtime` | The runtime against `greem-test-app`'s generated module and hand-written resolvers: queries, nested lists, interfaces, unions, null propagation in all three error behaviors, serial mutations, introspection, variables, depth limit, `@defer`, `@stream`, and the pulled payload stream (no work ahead of the consumer, cancellation on drop). |
| `cargo test -p greem-macros` | `#[greem::object]` per-object and set-based sugar, hints and plan routing. |
| `cargo test -p greem-reference` | The depth-first reference executor and its equivalence to the BFS on hand-picked queries. |
| `cargo test -p greem-compliance` | Generated code compiled from `schemas/property.graphql` through `build.rs`; property-based BFS≡DFS over generated documents, worlds and interleavings; the incremental fold property; breadth-first call counts; determinism; hand-written spec cases; depth, cancellation and panic evidence on a 2 MiB stack; 1,000 stream turns and 1,000 mutation roots. |
| `cargo test -p greem-example-axum` | The axum example: `build.rs` codegen, hand-written and sugared resolvers, a lookbehind hint, JSON and `multipart/mixed` responses over the router. |
| `cargo run -p greem-example-axum` | Serves `POST /graphql` on `127.0.0.1:8080`. |

`PROPTEST_CASES=5000 cargo test -p greem-compliance --test properties` is the long
run; the regression file under `greem-compliance/tests/` keeps every shrunk
counterexample found so far.

## Crates

| Crate | Role | Entry points |
| --- | --- | --- |
| `greem` | Runtime: resolver contract, execution tree, plan table, breadth-first executor, incremental delivery, HTTP types. | `Resolver`, `Outputs`, `Schema`, `Context`, `greem::http` |
| `greem-core` | What `greem-build` and `greem-macros` share: the GraphQL-name-to-identifier mapping. | `greem_core::ident` |
| `greem-build` | Schema compilation from `build.rs`: SDL → generated module in `OUT_DIR`. | `greem_build::compile`, `configure()` (`file_name`, `scalar`, `absent_aware`) |
| `greem-macros` | Per-type resolver sugar, re-exported by `greem`. | `#[greem::object]` |
| `greem-reference` | Unpublished naive depth-first executor, the oracle. | `greem_reference::execute` |
| `greem-compliance` | Unpublished harness: property schema, world, generators, evidence tests. | |
| `greem-test-app` | Unpublished: the module `greem-build` generates from its `schema.graphql`, and hand-written resolvers over it, shared by the runtime, macro and reference integration tests. | `schema`, `app::build_schema` |
| `examples/axum` | The one framework example. | `router`, `build_schema` |

## The contract a user writes against

One trait implementation per schema field on the Rust type that stands for the
parent object type (`greem/src/resolver.rs`):

```rust
impl Resolver<schema::User::posts, App> for User {
    type Output<'obj> = Vec<Post<'obj>> where Self: 'obj;
    async fn resolve<'obj, 'call>(parents: &'call [&'obj Self], args: &'obj Args<schema::User::posts>, ctx: &'obj Context<'obj, App>)
        -> Result<Vec<Self::Output<'obj>>, Error> where 'obj: 'call { ... }
}
```

- Parents are an index-aligned slice; outputs are index-aligned to it and may
  borrow the objects for `'obj`. A length mismatch is a framework error per parent.
- `Output<'obj>: Outputs<F::Type, C> + Send` are the only output bounds:
  completable as the field's type, and sendable so batches move between
  generations. Object types are `Send + Sync` (the `Resolver` supertraits):
  every field future of a scope borrows the same objects and is `Send`, so the
  borrowed objects must be `Sync`. `Outputs` has one impl, bridging to the sealed tag-side `Completes<T, C>` that codegen emits per
  type. `&T`, `Box<T>`, `Arc<T>` and `Result<T, Error>` objects delegate at the
  `Resolver` level; `Option`, lists, `Streamed<S>` and the scalars complete
  through greem's own tags (`Nullable<Ty>`, `List<Ty>`, `scalars::*`).
- List outputs: owned `Vec<T>`, `Box<[T]>` and `Arc<[T]>` (whose items are
  cloned out); borrowed `&[T]`, `&Vec<T>`, `&Box<[T]>` and `&Arc<[T]>`; and
  `Items<I>` for any other collection that iterates by value and by reference.
  One internal adapter (`exec/list.rs`) backs a single `List<Ty>` impl, and
  `Result<X, Error>` wraps any list output, `Streamed` included.
- Abstract positions: `As<types::User, T>` and `Either<A, B>`, one partition leaf
  per arm; repeated arms are separate scopes.
- An object fails as a whole only when returned as `Result<T, Error>`
  (forwarded through `&T`, `Box<T>` and `Arc<T>`). It is marked once, at its
  own position (a failed root is `data: null` with the error at the empty
  path), before any child scope exists. The `parent_error` hook that carries
  this is sealed, so every field's impl on one type agrees and codegen asks the
  first.
- `hints` declares accepted hint types; `plan` runs post-order at tree build
  and writes hints upward (`Planning::hint`, to the nearest accepting field
  above the writer, never the writer's own slot).
- `Context<'req, C>` is a per-invocation view: `app()`, `hint::<H>()`,
  `try_hint`. It borrows the application value and the frozen plan table for
  the request and shrinks to the resolver's `'obj`, so the signature is
  `ctx: &'obj Context<'obj, C>`. Inside an `async fn` Rust requires the
  lifetime spelled out (`&Context<'_, App>`); `#[greem::object]` inserts it, so
  sugared methods keep writing `&Context<App>`.
- Root values are supplied per request: `Roots { query, mutation }`. A schema
  with a `Mutation` type requires a mutation root that resolves every mutation
  field; `NoMutation` only satisfies schemas without one. This is the
  "missing resolvers are compile errors at the schema boundary" rule applied
  to roots.

`#[greem::object(schema = crate::schema, type = "User", context = App)]` emits
one `Resolver` impl per method, under the method's own `#[cfg]`/`#[cfg_attr]`
(`&self` methods join per object; a leading `parents` parameter is set-based); `#[greem(name)]`, `#[greem(hints = "field")]`
and `#[greem(plan = "field")]` as in ticket 11, each generated hook under its
hook method's own `#[cfg]`/`#[cfg_attr]`. See `examples/axum/src/main.rs`.

## What codegen emits

`greem-build/src/codegen.rs`, one file per schema, formatted with prettyplease:

- `types::<Name>` tags for every object, interface, union, enum and custom scalar.
- `<Object>::<field>` markers with `Field { Args, Type, NAME, SHAPE }`; `Type`
  is the tag expression (`List<Nullable<types::User>>`), `SHAPE` the static
  nullability per list level and for the innermost position.
- Public enums implementing `greem::Enum` (values, names, reverse lookup;
  trait items, so any value name compiles), `FromInput` and leaf completion; input objects with
  `FromInput` (`Maybe<T>` for `absent_aware` types; a single value coerces
  to a one-item list, but null at a non-null list is an error); argument structs under
  `__private::args::<Type>::<field>` (custom scalars read through
  `Scalar::from_input`). Names keep their GraphQL spelling; keywords become raw
  identifiers, and `self`, `Self`, `super`, `crate`, `types`, `__private`,
  `Schema` (the module's entry point) and any name already ending in `_` get a
  trailing underscore, so the mapping is injective; `#[greem::object]` uses
  the same function (`greem_core::ident`). Generated code and the
  macro's output name standard-library items by full path wherever schema or
  user names are in scope, so names like `Result`, `Vec` or `Send` cannot
  shadow them.
- Per object type: the `Completes<T, C>` impl bounded on every field's
  `Resolver`, with `walk` (the plan walk), `complete` (parent-error witness,
  then a child scope), `start_fields` (the static per-field dispatch into
  `greem::__private::field`) and `reference`.
- Per abstract type: `As` impls per member and sub-interface, `Either`,
  `Result`.
- Custom scalar codecs (`Codec::{Uuid, Json, String, I64}`; integers above
  `i64::MAX` travel as `Value::UInt`/`InputValue::UInt`, so `Json` keeps them) or the user's
  `impl Scalar for types::X`, whose `Value` is `Clone + Debug + Send + Sync`
  (argument and input-object structs derive `Clone` and `Debug`). Leaf
  completion is one emitter for built-in, enum and custom scalars. A leaf
  that completes to null at a non-null position (a custom scalar whose value
  is JSON null) is an execution error, `NULL_AT_NON_NULL`. `first_error`
  reports it too, so a stream never waits behind such an item; beneath
  `Nullable` the same null is a value.
- `__private::Info: SchemaInfo` (embedded SDL, greem-build's version as a
  literal for the skew check at `build()`, root tags, and `check_scalar`,
  which runs a custom scalar's codec by name so variables holding one are
  validated as request errors before anything executes) and
  `pub type Schema<C, Q, M> = greem::Schema<Info, C, Q, M>`.

The `@defer` and `@stream` directive definitions are appended to the SDL when
absent; apollo-compiler validates documents against them.

## Execution model

`greem/src/tree.rs`, `plan.rs`, `exec/*`.

1. **Tree build** (per request): `Schema::parse` validates once and returns an
   `Arc<Document>`; `execute*` coerces variables (apollo, then each custom scalar's codec), runs apollo's
   introspection for `__schema`/`__type`, then builds the execution tree lazily
   as the plan walk descends: one node per response key, fields collected per
   concrete type, whose own field definitions supply argument defaults
   (`CollectFields` with `@skip`/`@include`, defer usages and the
   stream directive resolved; their arguments follow argument coercion, so an
   unspecified argument or an unprovided variable takes the default from the
   directive's definition in the schema and an explicit null on `if` is a
   request error), the depth limit checked while descending.
2. **Plan walk**: generated `walk` visits every `(node, partition leaf)` the
   root Rust type can produce, creating one plan entry each (headers: keys,
   spans, child nodes, field sets by defer-usage set, hint slots; typed payload:
   the generated `Plan` enum with `Result<Args<F>, Error>` per field). `hints`
   runs on entry, `plan` on exit. The table is frozen behind an `Arc`.
3. **Generations**: a `Scope` (`exec/scope.rs`) is one object set at one plan
   entry for one field set. Its field futures are joined; completion writes
   columns and creates child scopes as `Frame`s (`self_cell`: owner = the
   output batch and per-field `Context` views, dependent = the child scope
   borrowing them). The top-level loop polls the whole tree one generation at a
   time; children created in a generation start in the next.
4. **Barrier** (`exec/barrier.rs`): close a finished serial root field, then
   for each released delivery group with no live scopes, collect errors,
   settle (null pass), mark alive objects, build the payload (`Payload<'p>`
   borrows the frames; the caller's encoder turns it into the stream's item in
   place, and the loop waits until the consumer pulls it, `exec/pull.rs`); ship finished stream
   item ranges; announce child groups. Then `advance` (`exec/run.rs`): release announced groups, start
   deferred field sets, turn buffered stream items into turns, retire shipped
   turns (children, slots and values are freed and the slot is reused by the
   next turn, so memory follows in-flight work rather than stream length),
   mark finished subtrees quiescent. A group whose parent position was nulled
   is dropped at announcement, so a sibling parent's stream on the same
   column is not held back.
5. **Mutations**: the root scope is serial: one root field at a time, each
   subtree run to completion, settled, and the next started only if the data
   survived (Propagate) or always (Null); HALT stops after the first error.

### Result storage

Columns (`exec/column.rs`) are per scope and per field: `level0` holds one
outermost list slot per object; each `Turn` holds the deeper list levels, the
innermost leaves or object links, and the child frames. Slots are
`Items | Null | Error(id) | Propagated | Pending`; a propagated slot keeps its
link so errors beneath it are still reported. Leaf values borrow the objects
(`Value<'a>`); `__typename` is synthesized from the scope's tag; introspection
values are pre-resolved columns. A non-finite `Float` output is an execution error
at its position (JSON has no NaN or infinity). Errors are recorded on the turn that owns the
slot with the object index and list indices, so a retired stream turn frees
its error records with its values; paths are rebuilt from the scopes' parent
links plus the field key. `errors` order is generation, then field, then
object; serial mutation roots come first, each root field's errors (its
subtree's included) before the next root field's.

### Null propagation and error behavior

`exec/settle.rs`: a subtree-rooted downward pass driven by the static shape;
a non-null failure returns to the nearest nullable ancestor which becomes
`Propagated`. `ErrorBehavior::Null` skips the pass, `Propagate` runs it,
`Halt` marks the group and keeps the first error at record time (the column
holding it may never complete), and wakes the loop so the next barrier ships
the halted group without waiting for its pending siblings: `data: null` plus
that error, or a failed `completed` entry for a deferred or streamed group.
A halted stream group fails as soon as it is released, even before its items
made a turn. Later work of the group is filtered, and scopes whose groups are
all dead go quiescent and are never polled again. Every pull, initial or
later and whatever the item's nullability, stops at the first failing item
and records it before anything else is pulled; later parents of a dead group
are not pulled at all. An item fails when the source yields an error or when
completing it would record one before any resolver runs
(`Completes::first_error`: a failed object, a leaf its scalar cannot
represent). Under `Propagate` the same check ends a stream's source at an
error its non-null item cannot absorb, one that reaches the item through
non-null positions only, so such an item never waits for the next one. Delivered data is never rewritten: a walk that
would cross a delivery boundary fails the group instead.

### Incremental delivery

- A delivery group instance is `(defer usage, object)` or `(stream field, parent object)`
  (`exec/state.rs`). Groups are reference-counted by the objects, scopes,
  drivers and child groups that name them; a finished group nobody references
  (terminal, or dead under a failed or dropped ancestor) is reclaimed by a
  sweep at each barrier and its slot reused, so the group table follows
  in-flight work rather than stream length (evidence: `MAX_LIVE_GROUPS` in the
  1,000-turn test and the abandoned-nested-groups test). Objects carry the pending groups they participate in; a
  deferred field set waits for release, then runs as a scope over the same objects. A set one
  fragment selects runs under that fragment's group. A shared field set
  (several fragments select it) runs under its own entry in the group table
  (`GroupKind::Shared`), which has no wire identity: it starts once any
  member fragment is released, ships under
  the id of the first member that completes, and keeps every member from
  completing until it has settled. One member failing does not touch it; its
  own failure fails every member; it is dropped once no member can deliver it. Its
  record outlives its objects until every member has completed, since a
  member may read the outcome later. Fields are grouped relative to the
  usages the producing field is delivered under, with the ancestor rule from
  the spec.
- `Streamed<S>` at the outermost list level becomes a driver (`exec/stream.rs`):
  `initialCount` items are pulled inside the immediate scope; the pump fills a
  capacity-bounded buffer while other work is pending; each turn is one
  set-based scope over every parent's buffered items; an item error at a
  non-null item type fails that parent's group and drops its source (under
  `Propagate` or `Halt`), while at a nullable item type, or under `Null`, it
  nulls the item and the source continues. The initial pull stops at such an
  error too, so a source that never yields again cannot hang the request. A parent's
  items ship once nothing beneath them is live for that parent's group, so
  nested streams and deferred groups under the items are released by that
  payload rather than holding it back, and once every earlier item of that
  parent has shipped (list order, whichever turn slot holds them). Groups
  under an item that has not shipped stay pending until it does. A scope is
  unfinished while a stream of its own has unshipped turns or an uncompleted
  group. A lazily drained list holds its parents' groups and never announces
  or completes them. A stream group is dropped at announcement when its
  list position did not survive settling (nulled by an item error, or never
  a list), as it is when the parent object was nulled. A `Vec` at
  a streamed field enters the same driver as a ready stream; a `Streamed` at a
  field that is not streamed drains in place.
- Ids are assigned at announcement in tree order, so output never depends on
  internal allocation order. One update result per barrier, with its
  incremental entries ordered so the entries that create positions come first
  (shallower paths, and a stream's items before data deferred on them);
  `hasNext` is false once no group is live; releasing a group counts as
  progress, so a fragment whose only content is a nested defer still leads to
  its descendants.
- A fragment nested inside another fragment but delivered inside stream items
  gets the stream group as its delivery parent and the enclosing fragment's
  group as an `after` dependency: it is announced with the items and released
  only once that enclosing group has completed; it never completes before
  that group either, successfully, failed or halted, even when all its
  fields are shared and it has no scope of its own to hold back. The enclosing fragment is
  the one the occurrence was collected under, not another fragment that
  merely selects the same field. If it failed before the announcement the
  nested fragment is dropped; if it fails afterwards the nested fragment
  completes with the same error. `IncrementalDelivery::Disabled` makes the
  tree ignore both directives: their arguments are not validated, though
  merged fields must still agree on `@stream` (its resolved arguments,
  however the directive is written). A fragment spread reached through
  several enclosing fragments is collected once per enclosing fragment, so
  each copy of its nested defers keeps its own dependency.
- Deferred groups that depend on a stream outside their own delivery (a
  fragment whose only fields are inside stream items) stay live until that
  stream ends; streams announced under a group never hold it back.

### Depth policy

`Schema::builder().max_depth(n)` (default 32) is checked at tree build:
composite nesting adds one, each nested `@defer` level and `@stream` add one, lists, fragments,
aliases, partition and introspection add none; mutations take the maximum over
roots. Rejection is a request error. Evidence (`greem-compliance/tests/depth.rs`):
completion at the limit and at 63 under a limit of 64, cancellation at the
deepest generation and a panicking deepest resolver with child-before-parent
drops, 1,000 stream turns at capacity 1 and 1,000 mutation roots with 30-deep
chains, all on a 2 MiB thread in a debug build. apollo's parser limits are
separate builder options (`parser_recursion_limit`, `parser_token_limit`);
apollo's introspection depth check runs unconditionally before execution and
has no setting.

### Reference executor and compliance

`greem-reference` walks depth-first with a one-element parent slice per object,
builds `serde_json::Value` directly and bubbles nulls by returning `Err` up the
recursion; it shares the tree, the plan walk and apollo's introspection with
the BFS and nothing else. Its per-tag support is the `Completes::reference`
method, which replaces the two support traits sketched in ticket 12 with one
hidden method. It exists only under greem's `reference-executor` feature:
codegen always emits it inside `greem::__private::reference!`, which keeps it
when the feature is on and drops it otherwise, so the same generated module
builds either way. `greem-reference` and `greem-compliance` enable the feature;
`cargo build -p greem` and `cargo test -p greem` run without it.

`greem-compliance/tests/properties.rs` generates documents from the property
schema (aliases, fragments, `@skip`/`@include` with variables, `@defer`,
`@stream`), worlds (object counts, failure map) and interleavings (per-call
yield counts under a single-threaded executor) and checks: BFS ≡ DFS on ordered
`data` and error multisets; HALT is one error from the Null-mode set; the
incremental stream (root lists and `drafts` may stream; stream capacity varies
from 1 up to the default, so streams split into turns that finish at different
barriers) folds to the Disabled result whenever no group failed and no
error sits beneath a propagated null (ticket 10's precondition, since work under
a null is dropped, not delivered). Ticket 11 adds a third precondition the
harness does not exercise: hint writers must not read their delivery group,
because a group-sensitive hint may legitimately change a field's result between
Enabled and Disabled without any error. The property generator never selects
the schema's one group-reading writer (`User.tag`), so the property is
established for hook-free documents; the hand-written case
`group_reading_hints_are_outside_the_fold_property_by_design` shows the
excluded behavior with the other two preconditions intact; BFS
call count never exceeds the reference's;
byte-identical output across runs and interleavings; the depth limit is exact
and shared with the reference.

## Deviations from the tickets

- `Context` carries a request lifetime (`Context<'req, C>`) so the per-field
  views are plain borrows; ticket 05 wrote it without one.
- Reference support is `Completes::reference` plus `reference_object` /
  `reference_field` helpers, not `ReferenceDispatch` / `ReferenceComplete`
  (ticket 12).
- Error ids are local to the column that owns the slot (ticket 08).
- A finished subtree is marked quiescent at `advance` so later polls and
  liveness checks skip it; without it 1,000 parked mutation roots were quadratic.
- Merged fields whose occurrences carry different `@stream` directives are a
  request error (graphql-js's rule; apollo-compiler does not check it).
- The cardinality failure is a framework error only; no debug assertion.

## Not in the skeleton

Left as the map's follow-ups: `#[derive(greem::Abstract)]` (designed in ticket
15), the graphql-js fixture port onto area schemas, `greem-bench`, the
`onError` wire attribute, SSE transport, the dataloader primitive,
tree caching, and per-generation polling efficiency (every poll traverses the
non-quiescent tree; a ready queue is the obvious next step).
