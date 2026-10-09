# Owned list outputs are kept and completed by reference

A resolver returning an owned `Arc<[T]>` used to pay one clone per item: nothing can move out of a shared slice, and nothing outlived completion for leaves to borrow from. Completion now keeps each batch of owned slices beside the values completed from them and completes the items through the borrowed `&Arc<[T]>` path, so items need to complete by reference rather than be `Clone`. Whatever owns the turn keeps them: the frame owner for turn 0, which lives as long as its scope, and the stream turn itself, which is built like a frame so that the slices retire with it, as [Parked chains bound execution depth](0005-parked-chains-bound-execution-depth.md) requires of turn storage.

## Considered options

- Documenting the zero-copy routes (`&Arc<[T]>` borrowed from the parent, `Arc<[Arc<T>]>` for objects) and leaving storage alone: every owned slice still clones.
- A shared `Arc<str>` leaf: helps only `Arc<[Arc<str>]>` and changes the public `Value`.
- Item handles, as the list's item type or as a leaf pointing into the kept slice: one item type per container, so `Option`, nested-list, abstract and object items each need a path of their own.
- Keeping a stream turn's outputs in its scope's frame owner: one owner per scope, but a streamed list of `Arc` rows would hold every row until the scope ends.
- An owner cell for every turn, turn 0 included: a stream driver is registered from inside completion and needs the frame's lifetime, which a cell's borrow cannot give.
- Letting kept outputs borrow the frame: a list appended through a shared borrow is invariant in what it holds, so only a second lifetime through every `Completes` impl would admit slices of borrowed items, which a `Vec` already moves out for free.

## Consequences

Owned `Arc<[T]>` requires `T: 'static` and `&T` to complete; an owned slice of items borrowing the parent no longer compiles. Every built-in and generated item type completes by reference, `&Result`, `&As`, `&Either`, `&Items` and borrows of borrows included; a custom scalar whose `Scalar::Value` is itself a reference would collide with the borrow-of-a-borrow leaf impl.

Kept slices live until their scope or turn ends instead of until completion, so with short items they cost more peak memory than the clones did (the `deferred` workload's 60k two-character tags: +2.4 MB), and with long ones less. An owned slice returned at a streamed field is kept by turn 0 and held whole until its scope ends; `Streamed` items still free turn by turn. Each stream turn costs two allocations for its owner and cell.
