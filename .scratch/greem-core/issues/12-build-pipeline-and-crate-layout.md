# How does SDL become generated code, and how does the runtime get the schema?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 01, 10, 11
Map: ../map.md

## Question

Specify greem-build's API (tonic_build-style configure/compile), SDL inputs, OUT_DIR + include! convention, what the generated module contains, how the compiled schema is embedded for runtime validation and introspection, custom scalar declaration/representation without a type mapping, and the crate layout (greem, greem-build, greem-codegen, greem-macros for `#[greem::object]`, adapters).

From ticket 05: inputs go through greem's own `InputValue` tree and a `FromInput` trait (`from_input(&InputValue) -> Result<Self, InputError>`), built from apollo-compiler's coercion output; codegen derives `FromInput` for arg structs, enums and input objects; greem ships impls for built-in scalars, `Option<T>`, `Vec<T>`. Specify `InputValue`, the coercion→`InputValue` boundary, and how a custom scalar's `FromInput`/`Outputs` impls are supplied by the user without a type mapping.

The earlier allocator-driven MSRV 1.100 rationale was superseded by [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md): its safe ownership probe compiles on 1.98 without allocator features. Set the actual minimum-version policy here after checking the selected dependencies; no version-policy change was inferred solely from the probe. Codegen emits, per object type, dense field indices and a `types::User::Plan` struct (grouped field set entries carrying converted `Args<F>`) used by the per-request (node, type) cache; and the generated `Completes` code implements `Scope::run` (static join of fields, partition, next-generation scopes). Decide where these generated items live in the module and that nothing apollo-typed is exposed.

Apply the generated lifetime/ownership interfaces and callback response boundary from [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md), rather than the superseded flat-loop sketch. Plan storage is decided in [How are eagerly built Plan entries stored and borrowed per request?](16-lazy-plan-storage.md).

From [How does post-hoc null propagation and onError work over the arena?](09-nullability-and-onerror-semantics.md): codegen emits a static per-field shape descriptor (list depth, non-null flag per level and for the leaf) referenced by the column header; the execution tree keeps a source span per merged selection so errors emit `locations`, which fixes what must be retained from the apollo document past the tree build.

From [How do @defer and @stream map onto generations and the arena?](10-incremental-delivery-in-generations.md): incremental delivery is in the skeleton. The per-(node, type) Plan splits its grouped field set by delivery group (a set of defer usages) and codegen emits one `Scope::run` per group over the same objects; an owned continuation scope kind (`run(self: Box<Self>, …)`) carries streamed tails; a generated `Completes` impl accepts `Streamed<S>` at list positions only; composite columns carry `tails`; the execution tree stores defer usages, stream usage, `initialCount` and `label` per node with `if` resolved at build like `@skip`/`@include`; the executor takes `IncrementalDelivery { Disabled, Enabled }` and the response primitive is a per-payload sink.

From [What can a field say to its ancestors during planning, and what does the skeleton ship?](11-planning-hooks.md): `Resolver<F, C>` gains two default methods, `hints(reg: &mut HintRegistry)` and `plan(plan: &mut Planning<'_, F, C>)`; the object macro routes `#[greem(hints = "…")]` / `#[greem(plan = "…")]` fns into them. Codegen emits, per type tag, a plan-walk method that recurses through every `Resolver::Output` and every `Either`/`As` arm so the whole tree is visited at build; it calls `hints` when a Plan entry is constructed and `plan` in post-order. Plan entries are built eagerly for every (node, partition leaf) and carry a `TypeId`-keyed hint slot vector next to the args. `Context<C>` is generated as a per-scope view (request context, node, Plan entry) exposing `ctx.app()` and `ctx.hint::<H>()`.

## Answer

Agreed with Iha Shin, 2026-09-28. Twenty-four questions over three rounds; all resolved as recommended except five where Iha chose differently: the embedded schema is shaped for a later apollo-free construction model (Q5), custom-scalar codecs are emitted by a build option rather than linked through a trait and cargo features (Q7), the minimum Rust version is a rolling policy (Q13), `Maybe<T>` inputs are opt-in per input object type (Q16), and the reference executor lives outside `greem` (Q18).

**Vocabulary** (now in `CONTEXT.md`): *schema compilation* is the build-time step from SDL to the *generated schema module*.

### Crates

1. **Workspace:** `greem` (runtime), `greem-build` (schema compilation; apollo-compiler and the code generator are private inside it, no separate `greem-codegen`), `greem-macros` (`#[greem::object]`, later the abstract derive), `examples/axum`, and `greem-reference` (`publish = false`; the depth-first reference executor, consumed only by the compliance crate). No adapter crate. Rejected: a shared codegen library (the macro is syntactic and never sees SDL); a `greem-axum` crate (out of scope).
2. **Cargo features on `greem`:** `macros` (default on; re-exports `greem-macros` so opt-out users drop the proc-macro dependency) and `reference-executor` (only the two hidden support traits, item 16). `greem::http` and `serde_json` are unconditional.
3. **Minimum Rust version:** rolling *stable minus two*, so `rust-version = "1.96"` today, edition 2024; CI builds the floor toolchain; bumps are free at any release. The dependency graph's own floor is 1.85 (indexmap 2). ADR 0002's 1.100 rationale is superseded and its status line says so; no new ADR.

### greem-build

4. **API:** `greem_build::compile("schema.graphql")` and `greem_build::configure()` with exactly: `out_dir` (default `OUT_DIR`), `emit_rerun_if_changed` (default true; greem-build prints one `cargo:rerun-if-changed` line per SDL file plus `build.rs`, since prost-build deliberately prints none and cargo's default crate-root watch is replaced once any line is printed), `compile(&[paths])` merging several SDL files into one schema, `scalar(name, Codec)` (item 12), `absent_aware(&[input object types])` (item 11), and hidden `reference_executor(bool)` (item 16). `compile` returns `Result<(), greem_build::Error>` whose `Display` is apollo's rendered diagnostics, so `build.rs` is `fn main() -> Result<(), Box<dyn Error>> { greem_build::compile("schema.graphql") }`. Generated code is `quote`d and formatted with `prettyplease` so `OUT_DIR` is readable.
5. **Output and include:** one file `OUT_DIR/greem.rs`; `greem::include_schema!()` expands to `include!(concat!(env!("OUT_DIR"), "/greem.rs"))` and the user wraps it in a module of their own name (`mod schema { greem::include_schema!(); }`). A file-name option for a second schema per crate is not shipped.

### Generated schema module

6. **Public surface:** type tags `types::User`, field markers `User::name`, input objects and enums with exact GraphQL spelling (ticket 05), `Schema<C>` with its builder. SDL descriptions become `///` doc comments. `@deprecated`, `@specifiedBy` and custom schema directives are introspection facts only; codegen emits no Rust attributes for them.
7. **Executor-facing surface:** `#[doc(hidden)] pub mod __private` holding per-field shape descriptors (ticket 09), dense field indices and Plan structs split by delivery group (tickets 08, 10), `Completes` impls including the `Streamed<S>` list impls, one `Scope::run` per delivery group plus the owned continuation kind, the plan walk and `hints`/`plan` calls (ticket 11), `Context<C>` view construction, the embedded schema item (item 8), the greem-build version constant, and reference-executor impls when requested. Every trait these implement is sealed in `greem`; the runtime helpers they call live in `greem::__private` (public, hidden, semver-exempt). Nothing apollo-typed appears anywhere in the generated file.
8. **Schema at runtime:** the generated module embeds the SDL text; `Schema::<C>::builder().query::<Q>().mutation::<M>().build()` parses and validates it once with apollo-compiler and returns `Result<Schema<C>, greem::SchemaError>`, after comparing the embedded greem-build version constant against `greem`'s so a version skew is a clear error, not a compile failure deep in generated code. `Schema<C>` is `Send + Sync + 'static`; apps share it behind `Arc`. The embedded item's shape is greem-build's private business: when greem grows its own validator, greem-build emits construction code for a greem-owned schema model instead and nothing user-visible changes (fog). Rejected now: hand-serializing apollo's `Schema` (no serde impls; every runtime use still needs apollo's value).
9. **Introspection** (graduated from fog): `__schema` and `__type` root selections are marked introspection nodes at tree build; the executor runs apollo's `introspection::partial_execute` once before generation 0 and writes the converted `Value`s into the root scope's columns as pre-resolved leaves, so they ride in whatever delivery group the selection sits in. `introspection::check_max_depth` runs at tree build. `__typename` stays synthesized from the scope's tag (ticket 08). The reference executor does the same.

### Inputs

10. **`InputValue`:** `Null | Bool(bool) | Int(i64) | Float(f64) | String(String) | Enum(String) | List(Vec<InputValue>) | Object(Vec<(String, InputValue)>)`, owned, source-ordered objects, `Enum` distinct from `String` so generated enums reject a string literal where the schema says enum (variables are re-tagged during coercion because the schema type is known). `FromInput for i32` range-checks `Int`. Rejected: `serde_json::Value` (loses the enum distinction, leaks `serde_json` into the API).
11. **Coercion boundary:** apollo's public `request::coerce_variable_values` coerces variables (its `serde_json_bytes` output is converted once per request into a name → `InputValue` map; `serde_json_bytes` stays internal, `Request.variables` is `serde_json::Value`); greem implements spec `CoerceArgumentValues` itself at Plan-entry construction over the literal `ast::Value`, substituting variables and schema defaults, then `FromInput`. Variable failures are request errors; argument failures are field errors at that selection (ticket 05). Apollo's own argument coercion is `pub(crate)` and unusable. Nullable inputs are `Option<T>` with absent and `null` both `None`; input object types listed in `absent_aware` get `greem::Maybe<T> { Absent, Null, Value(T) }` for their nullable, default-less fields instead. Per-argument schema coordinates are fog.
12. **Custom scalars:** one trait implemented on the generated tag, so there is no orphan problem and no link trait:

```rust
pub trait Scalar {
    type Value: Send + Sync;
    fn to_value(v: &Self::Value) -> Value<'_>;
    fn from_input(v: &InputValue) -> Result<Self::Value, InputError>;
}
```

Generated arg structs and input objects use `<types::DateTime as Scalar>::Value`; generated `Completes<V, C> for types::DateTime` is bounded on that `Value` plus the usual `&V`/`Result` delegation. The impl is either hand-written in the user's crate or emitted by `.scalar("UUID", greem_build::Codec::Uuid)`. Codecs shipped: `Uuid` (`uuid::Uuid`), `Json` (`serde_json::Value`), and the `String` and `I64` passthroughs; each documents the crate and version range its emitted code references, and a missing dependency is a plain compile error naming it. A custom scalar with neither is a missing-trait error at the schema boundary. Codec fixtures are greem-build integration tests. Rejected: a link trait with codecs behind `greem` cargo features; a Rust-type-name mapping in build.rs.
13. **Built-in scalars:** `Int` outputs from `i8`/`i16`/`i32`/`u8`/`u16` exactly and from `i64`/`u32`/`u64`/`isize`/`usize` with a range check raising an execution error at that position; `Float` from `f32`/`f64`; `String` from `String`/`&str`/`Cow<str>`/`Box<str>`; `Boolean` from `bool`; `ID` from the string types and every integer type. Arg struct types: `i32`, `f64`, `String`, `bool`, `String` for `ID` (integers coerced on input per spec). No silent truncation.
14. **Enums:** the generated enum is the only Rust type at an enum position in v0, output and input; users write `From` impls. A tag-side link like item 12 is the obvious extension and needs no data-model change.

### Runtime surface

15. **Entry points on `Schema<C>`:** `parse(&str) -> Result<Arc<Document>, RequestErrors>` (parse + validate once; opaque, `Send + Sync`, the anchor for the future tree cache and for a persisted-operations layer); `execute_with(roots, ctx, Operation, ExecuteOptions, sink: impl FnMut(Payload<'_>))`, `execute(...) -> ExecutionOutput` (owned bytes per payload), and `execute_request(roots, ctx, &Request, opts)` for the text path. `Operation<'a> { document: &'a Document, operation_name: Option<&'a str>, variables: serde_json::Value }`; `ExecuteOptions { error_behavior: ErrorBehavior, incremental: IncrementalDelivery }`. Request-level failures (parse, validation, variables, unknown operation) are delivered as a payload with `errors` and no `data`, flagged `kind: RequestError`, so adapters have one path and can still pick an HTTP status. `Request { query: Option<String>, operation_name, variables, extensions: serde_json::Value }`; ids, hashing and stores for persisted operations are out of scope. Parser `recursion_limit`/`token_limit` are builder options with apollo's defaults. `greem::http` holds these serde types, the single and incremental payload serialization, and a transport-agnostic `multipart/mixed` part encoder; `Accept` negotiation stays in the axum example.
16. **Reference executor support:** its two small traits (`ReferenceDispatch` for object types, `ReferenceComplete` for outputs) live hidden in `greem::__private`; greem-build emits their impls only under `reference_executor(true)`; the walk itself and the shared planning pass live in `greem-reference`. Rejected: emitting the impls unconditionally.
17. **`#[greem::object]` attributes:** `schema = crate::schema` (default) names the generated schema module, `type = "User"` (default: the impl target's identifier) names the GraphQL type, `context = C` as in ticket 05. The ticket-15 derive uses the same two attributes.

### Consequences for other tickets

Compliance (13): reference executor placement and the hidden build option. Skeleton (14): the build API, include convention, codecs, `greem::http`, the entry points, MSRV. Abstract-type derive (15): attribute convention. Plan storage (16): argument coercion runs at Plan-entry construction. Depth limits (17): parser limits are separate builder options.

Extension points: generated construction code for a greem-owned schema model (fog); per-argument `Maybe` coordinates (fog); more codecs (`chrono` was deliberately left out); a user-enum link; a file-name option for several schemas per crate.

## Amendment (from the resolved compliance harness)

Item 5's "a file-name option for a second schema per crate is not shipped" is overturned by [How is the BFS executor shown spec-correct?](13-compliance-harness.md#answer): `configure().file_name("lists.rs")` writes `OUT_DIR/lists.rs`, `greem::include_schema!("lists.rs")` includes it, and the argument-less forms keep the defaults. Each `configure()` carries its own `scalar`, `absent_aware` and `reference_executor` settings.
