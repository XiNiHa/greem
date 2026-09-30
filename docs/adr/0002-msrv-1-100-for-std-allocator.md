# MSRV 1.100 so the object arena uses the std `Allocator` API

Status: superseded. The ownership rationale was replaced by [Does the Outputs<Ty> trait encoding survive breadth-first execution?](../../.scratch/greem-core/issues/07-outputs-encoding-prototype.md#answer), and [How does SDL become generated code, and how does the runtime get the schema?](../../.scratch/greem-core/issues/12-build-pipeline-and-crate-layout.md#answer) set the minimum Rust version to a rolling stable-minus-two policy (1.96 at the time of writing) with no allocator-API need left. The original rationale below is historical.

Resolver outputs may borrow from their parents for the whole request, so every output is moved into a per-request append-only object arena that later generations extend while earlier references are live, and `Drop` must still run at request end. Rust 1.100 stabilizes `Allocator`, `Box::new_in` and `Vec::new_in`, which gives `Box<[T], &'req Bump>` with proper `Drop` on top of a bump allocator. We set the minimum supported Rust version to 1.100 from day one and build on beta until it ships, rather than starting on stable with bumpalo's own boxed type and migrating later.

## Considered options

- Stable Rust with `bumpalo::boxed::Box`: works today, but the arena type would leak into every scope signature and the migration would touch the executor's most delicate borrows twice.
- Per-type arenas (`typed_arena`): needs a type key, and `TypeId` requires `'static`, which request-lifetime outputs do not have.
