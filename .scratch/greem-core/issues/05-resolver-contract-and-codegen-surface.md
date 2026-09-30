# What does a user write for one field, and what does codegen generate around it?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 
Map: ../map.md

## Question

Fix the per-field primitive and its sugar. The set-based signature: what parents look like (slice? iterator? owned vs borrowed), how outputs align with parents (index-aligned Vec? iterator of (index, out)? map keyed by parent?), how per-parent errors are returned alongside successes, args and context parameters. The per-object adapter that desugars to it. The shape of the per-type sugar and of opting out. How a missing resolver surfaces as a compile error at the schema boundary, and what that error reads like. How the user's Rust type is inferred with no type mapping (root type given once; everything else follows trait bounds).

## Answer

The primitive, one impl per schema field on the user's type:

```rust
pub trait Field { type Args: FromInput; type Type; }
pub type Args<F> = <F as Field>::Args;

#[diagnostic::on_unimplemented(
    message = "no resolver for GraphQL field `{F}` with context `{C}`",
    label = "implement `greem::Resolver<{F}, {C}>` for this type")]
pub trait Resolver<F: Field, C = ()>: Send + Sync {
    type Output<'a>: Outputs<F::Type, C> + Send where Self: 'a;
    fn resolve<'a>(
        parents: &'a [&'a Self], args: &'a Args<F>, ctx: &'a Context<C>,
    ) -> impl Future<Output = Result<Vec<Self::Output<'a>>, Error>> + Send + 'a;
}
```

- **Parents**: an index-aligned slice of references. Every scope's objects are references into a request arena; owned outputs are moved into the arena first. Outputs may borrow from parents for the request lifetime (`name` → `&str`, `friends` → views into `self`). Consequence for the executor: the arena and scope erasure are request-lifetime-parameterized and cannot use `Any`.
- **Output**: index-aligned to parents. `Err` from the future fails every position. Per-position errors are expressed by choosing `Output = Result<T, Error>` (blanket `Outputs` impl). Length mismatch = framework error attributed to every position, never a panic.
- **Completability bound**: on the associated type (`Output<'a>: Outputs<F::Type, C>`), not on codegen's where clauses. Wrong output type errors at the impl site; recursive object types don't cycle through the generated blanket `Outputs` impl. Cost: a missing resolver also errors at every impl whose output mentions that object type. Ticket 07 confirms rustc behaves this way.
- **Error**: one concrete `greem::Error` (message, extensions; path filled by the executor) with blanket `From<E: std::error::Error + Send + Sync + 'static>`.
- **Args**: one owned value per selection (not per object). Generated concrete arg structs, enums and input objects derive greem's `FromInput`, converting from greem's own `InputValue` tree built from apollo-compiler's coercion output (apollo types stay out of the public API). `from_input` returns `Result<Self, InputError>`; only custom scalars can fail; failure is a field error at that selection.
- **Context**: `Context<C>`, `C: Send + Sync`, a type parameter on `Resolver`, `ObjectResolver`, `Outputs` and `Schema`, default `()`. User value via `ctx.app() -> &C` (no `Deref`, so future greem methods can't shadow user fields). An impl may be generic in `C` only if every field of every object type in its output is; apps pick one `C`.
- **Per-object sugar**: second trait `ObjectResolver<F, C = ()>` with `async fn resolve(&self, args: &Args<F>, ctx: &Context<C>) -> Result<Self::Output<'_>, Error>`; one blanket impl in greem desugars it to a set-based impl (concurrent join over parents) with `Output = Result<_, Error>`. Coherence forbids implementing both for one field. Macro-free, hence fully opt-out-able.
- **`#[greem::object(context = AppCtx)]`** (proc-macro crate, ships in the skeleton), purely syntactic on inherent impl blocks, any number per type: method name snake_case → camelCase with `#[greem(name = "…")]` override; `&self` receiver → `ObjectResolver`, receiver-less first param `parents` → `Resolver`; positional params with optional tail `(&self)`, `(&self, args)`, `(&self, args, ctx)` and the macro supplies `&Args<marker>` / `&Context<AppCtx>`; `async` optional (sync bodies wrapped); literal `Result<T>` (and for set-based, `Vec<T>`) layers stripped to find `Output`, anything else wrapped in `Ok`; `context =` omitted only for `C = ()`.
- **Naming** preserves GraphQL spelling exactly (case conversion is lossy: `User`/`user`, `name`/`Name` are distinct GraphQL names). Field markers `schema::User::name`; type tags `schema::types::User`; input objects/enums `schema::CreateUserInput`, `schema::Role`; args reached via `greem::Args<F>`, no second generated item. Generated module carries `#![allow(non_snake_case, non_camel_case_types)]`. Keywords → raw identifiers; `self`, `Self`, `super`, `crate` and the reserved `types` get a trailing underscore. That is the only mangling.
- **Schema boundary**: `schema::Schema::<C>::builder().query::<Q>().mutation::<M>().build()` — the one place root types are named, bounded `Q: Outputs<types::Query, C>`, `M: Outputs<types::Mutation, C>`; everything else is inferred through `Output`. `execute(Roots { query, mutation }, ctx: C, request)` takes root values per request; `greem::NoMutation` is the default `M`. A missing resolver reads as "no resolver for GraphQL field `schema::User::name` with context `AppCtx`" via `on_unimplemented`, with rustc's chain pointing at the boundary.
- **Bounds**: objects `Send + Sync`; outputs `Send`; no `'static` anywhere unless ticket 08's arena demands it; no GAT on `Args` or `Field`.

Rejected: iterator/newtype parent sets; `Vec<Result<_>>` output shape; user-generic error type; serde for inputs; type-erased context bag; per-field generated traits; codegen where-clause bounds; snake_cased generated modules; single root type for query+mutation; `Deref` to the user context.

## Amendment (from ticket 06)

The generated blanket `impl<T: Resolver<…>> Outputs<types::User, C> for T` violates the orphan rule (E0210). The mechanism: generated `impl<T: Resolver<…>, C> Completes<T, C> for types::User` (tag as `Self`) plus a single bridge in greem, `impl<T, Ty: Completes<T, C>, C> Outputs<Ty, C> for T`. Everything user-facing above stands. The `Result<T, Error>` / `&T` output blankets become `Resolver`-level delegations, not `Outputs` impls. See [06](06-abstract-types-without-type-mapping.md).

## Amendment (from the resolved Outputs encoding prototype)

The current contract revisions and their evidence are recorded in [Does the Outputs<Ty> trait encoding survive breadth-first execution?](07-outputs-encoding-prototype.md#answer). Its resolution supersedes the relevant adapter, error-delegation, lifetime, scope-identity, ownership and response sketches above; the original discussion is retained as history.
