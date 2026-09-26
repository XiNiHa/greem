# Arena / executor ownership probes

Throwaway fact-finding only. These do **not** establish an executable `Outputs<Ty>` implementation or prove a production arena sound.

Observed with `rustc 1.100.0-nightly (8925ea358 2026-08-20)`, cached bumpalo 3.20.3, futures 0.3.34, and typed-arena 2.0.2. Run from the repository root:

```sh
cargo +nightly run --offline --manifest-path .scratch/greem-core/prototypes/outputs/arena/Cargo.toml --bin plan
cargo +nightly run --offline --manifest-path .scratch/greem-core/prototypes/outputs/arena/Cargo.toml --bin generations
cargo +nightly check --offline --manifest-path .scratch/greem-core/prototypes/outputs/arena/Cargo.toml --bin bump_send
cargo +nightly check --offline --manifest-path .scratch/greem-core/prototypes/outputs/arena/Cargo.toml --bin typed
```

The first two pass. The last two deliberately fail; therefore checking all binaries at once is expected to fail.

## Facts

- `plan`: A fixed table of `OnceLock<Box<dyn Any + Send + Sync>>` allows lazy plan initialization through a shared borrow. Previously returned plan references remain usable while another slot initializes. This needs stable slots, e.g. slots assigned per existing tree node × generated GraphQL object type. This probe does not support concurrent growth of the outer table. A later design must specify slot assignment, type/slot consistency, error handling, and whether abstract tree expansion requires new slots.
- `bump_send`: `Bump` implements Send, but not Sync. Consequently `&Bump` and `Box<[String], &Bump>` are not Send. Merely joining futures on one thread does not satisfy an explicit Send bound. Carrying these boxes in scopes or across awaits therefore conflicts with Send scopes/futures.
- `typed`: Allocating boxed `dyn Scope<'req>` into typed-arena makes the loop itself type-check, but its owning caller fails drop checking: dropping the arena may use the same borrow tied to its lifetime. Simply swapping in a dropping typed arena does not close the ownership design.
- std `Allocator` remains marked unstable (`allocator_api`, issue 32838) in the installed 1.100 nightly source. Bumpalo's std `Allocator` implementation is gated by its `allocator_api` feature; this probe enables that Cargo feature and the relevant consumer enables `#![feature(allocator_api)]`. This installed August 20 snapshot predates the current September 26 date; it establishes only this snapshot's feature requirement, not the stabilization status of current beta or final Rust 1.100. No current release research was performed here.

## Safe alternative demonstrated, with changed execution ownership

`generations` uses no unsafe. `Scope::run(&self)` returns children borrowing that particular scope borrow; it does not require one universal `&'req self`. Each generation owns its scopes in an async frame, joins their Send futures, then awaits the next generation while retaining the parent frame. The runtime is breadth-first in when resolvers execute, but recursively nested in retained storage.

The executable includes a non-static reference into request-owned text and a child borrowing its parent's owned String. It asserts the executor future is Send and verifies on successful completion that child Drop runs before parent Drop (the child destructor reads its parent's string). It demonstrates trait object dispatch and lifetime ownership only: no Outputs trait, list/null shape, result arena, mutation scheduling, cancellation test, panic test, or bump-backed objects.

Costs/limits: one retained boxed async frame per generation, eventual deep polling/drop chains, and shorter nested lifetimes in the executor interface. A final response borrowing retained objects must be serialized before the frames unwind or use another ownership arrangement; this probe does not implement serialization. A strict flat loop with references all lasting for the full request remains unresolved.

## Recommended decision seam

Separate two decisions: (1) parent-storage ownership and duration, (2) allocation backend. The safe nested-frame experiment is one viable ownership direction to discuss, not a drop-in implementation of the current map. Keeping a flat loop and request-wide borrowed outputs likely requires an encapsulated lifetime/ownership arena mechanism; this experiment neither proves that unsafe is unavoidable nor supplies that mechanism.

A future unsafe design must establish stable addresses, prevent owner removal while borrowed, synchronize allocation if shared by Send futures, and enforce safe destruction dependencies including cancellation/unwind. Reverse insertion order alone is insufficient for an unrestricted arena API: interior mutability can introduce references from older objects to newer objects. The executor's generated completion interface may offer a narrower causal ownership discipline, but that requires an explicit contract and review.
