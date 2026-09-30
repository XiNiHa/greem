# Columnar result arena instead of a response tree

The breadth-first executor resolves one field across a whole scope at a time, so its natural output is one column per field, not one object per parent. We record every resolved value, null, error and list shape in a per-request arena laid out per scope and per field, with links both from a parent column to its child scope and from each child object back to its parent position, and serialize it once at the end through a `serde::Serialize` view.

## Considered options

- A JSON-like tree built during execution: simplest, but every composite field would have to fan its column back out into per-parent objects on every generation, and post-hoc null propagation, error paths and `@defer` subtrees would all have to re-walk a tree that was never indexed by scope.
- Column slots as a recursive per-parent list enum: rejected for an allocation per list node; nested lists are flat per-level offset arrays instead.

## Consequences

Error paths are not stored; they are rebuilt at serialization from the parent links. `__typename` is not stored; it comes from the scope's type. Any scope subtree can be serialized on its own, which is what incremental delivery needs.
