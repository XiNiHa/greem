# Abstract partition identity probe

The generic completion implementation for `Either<A, B>` knows two independently
typed completion branches. Even when a caller instantiates both with
`As<User, T>`, its body must type-check for arbitrary valid `A` and `B`. Matching
GraphQL tags does not supply a Rust type-equality proof.

Checked with `rustc 1.98.0 (88d9e12ae 2026-08-18)`, edition 2024:

| Probe | Result | Fact established |
| --- | --- | --- |
| `merge.rs` | E0277 | A generic `Vec<&A>` cannot receive `&B`. |
| `overlap.rs` | E0119 | A special same-type `Either<T, T>` impl overlaps the general `Either<A, B>` impl. |
| `normalize.rs` | Compiles and runs | A caller with a statically known repeated arm type can normalize into one member representation, including borrowed objects. |

These small probes expose the obstacles in this design; they are not a proof
about all possible Rust APIs. No automatic coalescing mechanism was established
under the stated constraints: ordinary stable generic completion, no extra
type-equality witness or type mapping, no specialization, no `'static` requirement,
and no erased-pointer casts.

## Concrete correction for the integrated probe

Partition each completed parent scope by the generated representation's leaf
path. A leaf determines its GraphQL member tag and concrete Rust type; repeated
leaves need not be coalesced. In particular,
`Either<As<User, T>, As<User, T>>` may generate two nonempty scopes and two calls
to a selected `T` field resolver. State the batching guarantee as once per
nonempty generated scope, not once per Rust type per generation.

For identity bookkeeping, use the parent scope instance, field tree node, and
partition leaf path. GraphQL tag and Rust type remain properties of the typed
leaf; they are not sufficient to uniquely identify a runtime scope. This also
avoids accidentally promising merging across distinct parent scopes.

A caller needing one batch can normalize repeated arms before returning the
resolver output, as `normalize.rs` demonstrates. That needs neither a mapping
nor runtime type comparison. A future user-enum derive may normalize variants
when their equality is explicitly available, but identical-looking Rust type
syntax is not a general semantic type-equality oracle (aliases matter).

Wrapping both branches in a synthetic enum and delegating its `Resolver` does
not by itself recover the original guarantee: generic delegation still splits
the `A` and `B` calls. Type-name strings or function addresses do not provide the
safe type-equality witness required to merge typed parent slices.

This is a newly exposed contract choice to put to the human, not an accepted
change to the map's earlier batching guarantee.
