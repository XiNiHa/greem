# Lookbehind planning runs once at tree build over every statically known abstract arm, and hints may shape results

Cardinal plans concrete subtrees before execution and re-plans each abstract subtree only after its parent resolves, with ancestors sealed, because a dynamic language cannot know which concrete types an abstract field will yield. greem's resolver output types name their abstract arms statically (`Either<As<User, U>, As<Post, P>>`, or a derived enum), so codegen walks the Rust type chain from the root value through every arm at tree build and runs one synchronous, post-order planning pass per request before generation 0. A hint written under an interface position therefore reaches the ancestors above it, at the cost of speculative hints for arms that yield no objects. Hints are typed by the accepting ancestor's resolver, addressed by type to the nearest accepting field, and delivered through a per-invocation `Context` view. They are part of a field's semantics: a resolver may return different data depending on what its descendants asked for, so the reference executor runs the identical pass and no "defaults only" mode exists.

## Considered options

- Cardinal-style lazy re-planning after partition: exact knowledge of which types appeared, but hints from under an abstract position can never reach above it, which is the case lookbehind exists for.
- Advisory hints (a resolver must be correct with the hint's `Default`): a stronger equivalence harness, rejected in favour of letting hints carry meaning.
- A loader/preload primitive in the pass: deferred until real resolvers show whether set-based resolution leaves an N+1 worth it.

## Consequences

Every Plan entry exists before execution, so Plan storage is a finite per-request table rather than lazily discovered slots, and Plan identity is (node, partition leaf). `Context<C>` is a per-scope view, not a shared request value. The incremental fold-and-compare property holds only for documents whose hint writers ignore their delivery group.
