# How does post-hoc null propagation and onError work over the arena?

Type: grilling
Status: resolved
Assignee: Iha Shin
Blocked by: 02, 08
Map: ../map.md

## Question

Design the final pass: how per-field errors are recorded in the arena, how the pass locates them, bubbles nulls to the nearest nullable ancestor, and emits error paths; what PROPAGATE / no-propagate / HALT each mean for the executor loop and the final pass; how mutation root fields serialize and early-terminate in a generation model; what the skeleton implements vs leaves as an extension point.

From ticket 08: errors are one per-request `Vec<ErrorRecord>` (message, extensions, origin scope + object index + field); slots are `Leaf | Null | Error(id)` in leaf columns and `Items(range) | Null | Error(id)` / `Object(idx) | Null | Error(id)` in list-shape levels; paths are reconstructed lazily by walking parent links and shapes. The null pass rewrites slots in place walking child → parent over the bidirectional links. HALT is a flag observed at the generation barrier; mid-generation cancellation is yours to add behind the same flag if wanted. Mutations run one root scope chain per top-level field, serially, in a shared arena.

## Answer

Agreed with Iha Shin, 2026-09-26. Ten questions, all resolved as recommended.

**Type-level fact that shapes everything.** In the accepted encoding (integrated prototype, `Nullable<Ty>` tag), `Option<T>` only completes at nullable positions, so a resolver cannot return an intentional null at a non-null position; that is a compile error. The spec's "cannot return null for non-nullable field" error class does not exist in greem, and the compliance harness treats that test class as vacuous. A non-null position becomes null only through an **execution error**: a `Result::Err` from a resolver, a `parent_error` object failure recorded at the parent slot, or a framework-detected error.

1. **Nullability source.** Codegen emits one static shape descriptor per field: list depth, one non-null flag per list level, one for the leaf/object. The column header points at it. Nested lists are handled level by level (`[[Int!]]!` = depth 2, levels `[non-null, nullable]`, leaf non-null). The executor never consults the compiled schema for nullability.
2. **Slot invariant.** Completion never checks nullability at write time. `Null` means an intentional, always-legal null; `Error(id)` means an execution error was raised at exactly this position. `Error(id)` serializes as `null` in every mode. "One error per response position" holds by construction (one slot, one id).
3. **The pass.** Error-driven upward walk. For each `ErrorRecord`, start at its origin slot and walk parent links one level at a time (slot → enclosing list levels → column → object → its parent slot …). At the first nullable position, rewrite that slot to `Propagated(id)` and stop. On reaching a slot already `Error`/`Propagated`, stop (already handled). On reaching the root object with no nullable position, `data` becomes `null`. Walks are mutually independent, so the outcome is order-independent; cost O(errors × depth); zero when there are no errors. Serialization stays a plain downward walk.
4. **`Propagated(id)`** is a fourth slot variant: serializes as `null`, records which error nulled it. The error's `path` stays at the origin. Incremental delivery (ticket 10) uses it to drop deferred work beneath a propagated null.
5. **`errors` order and suppression.** Deterministic: each `Scope::run` collects its errors in field-then-object order; the barrier appends per scope in generation order. Every raised error is reported; no RFC 2 (#1184) sibling suppression.
6. **HALT.** Flag observed at the generation barrier (ticket 08). Response is `data: null` plus exactly the first recorded error; the arena is neither null-passed nor serialized. No mid-generation abort in the skeleton; the flag is the single point a cancelable join would consult later.
7. **Mutations.** After each serial root chain finishes, run the pass rooted at that chain's root slot. PROPAGATE: if `data` is nulled, skip remaining root fields (matches graphql-js). HALT: stop after the chain that recorded the first error. NULL: always continue. The pass API is therefore subtree-rooted from day one, the same shape ticket 10 needs.
8. **`ErrorRecord`.** message, extensions, origin (scope, object index, field), plus source spans so `locations` is emitted (the execution tree keeps a span per merged selection; a merged response key emits every contributing span). No `pathNonNull` in the skeleton; the shape descriptors make it an additive change. Framework-originated errors carry a stable `extensions.code`; user errors are untouched.
9. **Cardinality failures** (resolver returns a `Vec` of the wrong length): one framework error per parent object at that field, sharing a message; execution continues. A `debug_assertions` panic is permitted for development loudness.
10. **Skeleton ships all three modes.** The executor takes `ErrorBehavior { Null, Propagate, Halt }` (default `Propagate`) from day one: NULL skips the pass, PROPAGATE runs it, HALT is the barrier flag. Only the wire-level `onError` request attribute plumbing stays out of scope. The map's Out-of-scope line is amended accordingly.

Extension points: cancelable generation join behind the HALT flag; `pathNonNull`; RFC 2 sibling suppression; semantic-nullability directives (out of scope).
