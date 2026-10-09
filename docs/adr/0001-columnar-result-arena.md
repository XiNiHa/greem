# Columnar result arena instead of a response tree

The breadth-first executor resolves one field across a whole scope at a time, so its natural output is one column per field, not one object per parent. We record every resolved value, null, error and list shape in a per-request arena laid out per scope and per field, with links both from a parent column to its child scope and from each child object back to its parent position, and serialize it once at the end through a `serde::Serialize` view.

## Considered options

- A JSON-like tree built during execution: simplest, but every composite field would have to fan its column back out into per-parent objects on every generation, and post-hoc null propagation, error paths and `@defer` subtrees would all have to re-walk a tree that was never indexed by scope.
- Column slots as a recursive per-parent list enum: rejected for an allocation per list node; nested lists are flat per-level offset arrays instead.

## Consequences

Object paths are not stored. Each scope links to its parent scope and field, and each object keeps only its parent index and list indices, so a path is rebuilt by walking up when an error, a deferred or streamed group, or an incremental entry needs one. Under Halt that happens when the error is recorded, because its column may still be pending. `__typename` is not stored; it comes from the scope's type. Leaf values are not copied out of the outputs they come from: they borrow the scope's objects, or owned list outputs kept beside them ([Owned list outputs are kept and completed by reference](0009-owned-list-outputs-are-kept-and-completed-by-reference.md)). Any scope's data can be serialized on its own, which is what incremental delivery needs; errors and `subPath` are worked out at the barrier, before serialization.
