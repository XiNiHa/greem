# How do Cardinal and Grafast do bottom-up (lookbehind) planning concretely?

Type: research
Status: resolved
Blocked by: 
Map: ../map.md

## Question

Read Shopify's Cardinal PoC (github.com/gmac/graphql-breadth-exec) and graphql-ruby's new execution module docs (graphql-ruby.org/execution/next.html), plus Grafast's planning docs (grafast.org). Capture concretely: the primitives a field can register during planning (preloads, notes, steps), when the planning pass runs relative to tree building and execution, how it handles abstract types that are not statically resolvable, how planned preloads are delivered to resolvers at execution time, and how lists and arguments participate. Note anything in these designs that assumes synchronous execution or a dynamic language and would not transfer to Rust as-is.

## Answer

- Only Cardinal has a per-request lookbehind pass: `build_execution_tree(scope).reverse_each { |f| f.resolver.plan(f, ctx) }` — bottom-up is literally reverse construction order, run once per root scope before the first resolve, with child scopes still empty of objects.
- Primitives: `preload(loader, args:, keys:)` on field or scope (keyless form binds to the element's `objects` at execution time), `on_preload {}` (sync just-in-time hook when objects exist), a freeform `attributes` hash on fields/scopes for notes and caches, `mutable_arguments`, and `planning_root`/`allows_preload?` for navigating up. A `:preloading → :executing → :locked` state machine raises `LazySequencingError` on misuse.
- Abstract positions are omitted from the tree; after the parent field resolves, objects are bucketed by concrete type, one concrete scope per type is built, and phases 1–2 re-run on that sub-tree. Ancestors are sealed, so `planning_root` is the highest scope that still accepts preloads.
- Delivery is manual: the preload promise's `.then` writes into `attributes`; the resolver reads it. The executor drains the scope queue, then runs every loader once and resumes queued elements. Lists are flattened into the child scope's `objects` at result-build time; arguments are coerced per selection at tree build and are available in `plan`.
- graphql-ruby `Execution::Next` (Cardinal's open-source descendant) has no `plan`/`preload`/`attributes` at all — only top-down `Lookahead` — and pushes batching onto `resolve_batch` + Dataloader.
- Grafast plans once per operation, breadth-first by depth, values-blind; its "children inform ancestors" effect comes from `$parent.get(attr)` mutating the parent step, `deduplicate`/`deduplicatedWith` merging, and dependents-first `optimize`. It plans every possible concrete type up front (`planType` → `$__typename` + `planForType(t)`) and skips non-matching entries per bucket at runtime; delivery is positional (`details.values[depIdx].at(i)`).
- Not transferable as-is: Cardinal's untyped `attributes` bag and `.then`-closure delivery (needs typed slots / index-based delivery), runtime sequencing errors (typestate instead), ancestor mutation during planning (arena indices), fiber-based async; graphql-ruby's fiber-parked steps and identity-keyed side tables; Grafast's plan cache, all-types-up-front polymorphism, heap-DAG rewriting, and `nextTick` batching.
- Details, excerpts and citations: [research/lookbehind-planning-in-practice.md](../research/lookbehind-planning-in-practice.md).
