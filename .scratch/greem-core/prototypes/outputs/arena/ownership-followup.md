# Flat-loop ownership follow-up

No sound general-purpose flat-loop arena implementation was established in this bounded investigation. Cached candidates were bumpalo and typed-arena; no external library survey was performed.

## Why reverse Drop order is not a sufficient safe API contract

A hypothetical safe `alloc<T: Send + Sync + 'req>(&'req self, value: T) -> &'req T` can publish references that safe interior mutability stores into older objects. Both newer-to-older and older-to-newer destructor dependencies then become possible. A topological destruction order need not exist.

`src/bin/drop_cycle.rs` concretely constructs `A<'a> { peer: OnceLock<&'a B<'a>>, name: String }` and `B<'a> { peer: &'a A<'a>, name: String }`; both implement Drop that reads the peer's String. Both satisfy Send + Sync. Allocate A, allocate B referencing A, then set A.peer to B. Dropping either one completely before the other invalidates data read by the later destructor. Keeping raw allocation bytes alive is insufficient after String's destructor frees its buffer.

The runnable probe intentionally leaks both allocations with Box::leak, which is safe; it demonstrates representable cyclic dependencies, not an unsafe destruction experiment:

```sh
cargo +nightly run --offline --manifest-path .scratch/greem-core/prototypes/outputs/arena/Cargo.toml --bin drop_cycle
```

This is not a demonstration that the precise current Resolver GAT signature admits every such cycle. Its narrower lifetimes may prevent escaping specific references. It is a counterexample to treating arbitrary `T: Send + Sync + 'req` plus reverse allocation order as a complete arena safety argument. A proposed executor interface must prove its actual reference restrictions, including user-provided resolvers, context, interior mutability, and borrowed outputs with Drop.

## Contracts a narrowly unsafe flat executor would need

1. **Stable publication:** allocations are pinned at stable addresses before publishing shared references. No removal, replacement, reset, mutable-reference alias, or reallocation of an allocation while references survive. Moving owning boxes is fine; moving their payloads is not.
2. **Lifetime boundary:** all exposed object and result references end before owners are destroyed. Hiding raw pointers in a destructor list only suppresses compiler checks; it does not establish this boundary. A higher-ranked scoped callback can prevent references escaping an owner, but does not by itself establish destruction order within that owner.
3. **Dependencies:** every destructor's borrowed dependencies stay alive until it finishes. Merely recording birth generation or reverse insertion order works only after proving references cannot introduce opposite-direction or same-generation cycles. If unsafe obligations are imposed on arbitrary output implementors, the relevant API/trait itself must express that obligation; generated internals cannot assume it from an ordinary safe trait.
4. **Cancellation:** pending futures and temporary output values drop before any storage they may reference. Cleanup cannot rely on the successful return path. Partially completed joined futures and values created before registration need RAII ownership throughout handoff.
5. **Unwind:** cleanup must cover partial construction and ordinary panic unwinding. If a destructor panics, a cleanup guard must account for remaining storage and references. No library can promise all destructors run after process abort or a double panic.
6. **Concurrency:** allocated boxes and the storage they capture must satisfy the actual Send/Sync requirements. Raw-pointer erasure does not justify unsafe Send. A synchronized allocator wrapper would need a separately reviewed Allocator implementation; it would solve allocation synchronization, not lifetime/drop-order obligations.
7. **Response:** a borrowed Response must remain tied to live object owners. It may be serialized inside a scoped owner callback. It cannot outlive the dropping arena or generation frames. Returning an owner-plus-borrowed-view package introduces another self-reference problem unless the public API borrows the view from an already established owner.

## Concrete alternatives to take to the human

- **Safe nested generation frames:** already executable, breadth-first scheduling, Send futures, ordinary Drop ordering enforced by Rust. Keeps the ancestors in nested frames; serialize while retained. Does not preserve the literal flat loop or one universal request lifetime. The updated probe also polls a pending child once and then cancels the executor; its child Drop reads parent data and runs before parent Drop, with both counters verified.
- **Flat loop with owned dependency handles:** change output/storage access to owner-carrying handles, and borrow values only through shorter callback/access lifetimes. Parent owners can then be retained explicitly. Supporting arbitrary self-borrowing output families may require a reviewed owner/dependent container implementation. This is a design direction, not a compiled solution here, and changes the original Vec<&'req T> interface.
- **Flat loop with request-wide references:** retain the chosen external shape, but keep ownership unresolved until a concrete scoped arena interface proves the above contracts. Restricting destructor behavior or reference escape would be an explicit user-facing constraint; “a little unsafe inside the arena” alone is not enough.

No complete implementation, safety proof, borrowed Response implementation, or production library recommendation is provided by these probes.
