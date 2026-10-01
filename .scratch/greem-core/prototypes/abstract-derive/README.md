# abstract-derive coherence probe

Throwaway rustc coherence probe for greem's abstract-type (union/interface) completion design.
`greem/` is the runtime crate, `app/src/shared.rs` is the "generated + user" code, and each
`app/examples/p*.rs` pulls it in via `#[path]` so every probe is one self-contained crate
(the blanket impls under test target the local `types::SearchResult`, so they must live in the
same crate as the As/Either impls).

Toolchain: `rustc 1.98.0 (88d9e12ae 2026-08-18)`, `cargo 1.98.0 (797e8a9bc 2026-08-05)`, edition 2024.

Run one probe: `cargo build --example p2` (from this directory).

| Probe | Result | Error code | rustc note | Meaning |
|---|---|---|---|---|
| P1  `p1`  | compiles | – | – | Derive-style direct `Completes<Hit<'a, X>, C> for SearchResult` with per-variant `Outputs` where-clauses coexists with the generated `As`/`Either` impls; `Hit`, `As<User, MyUser>` and `Either<As<User,_>, As<Post,_>>` all satisfy `Outputs<SearchResult, ()>`. |
| P1b `p1b` | fails (only on `check2`) | E0277 | `required for \`Hit<'static, MyUser>\` to implement \`Outputs<SearchResult, String>\`` (chain: `&MyPost: Resolver<PostTitle, String>` -> `Post: Completes<&MyPost, String>` -> `&MyPost: Outputs<Post, String>`) | The C = () call still type-checks (single error reported); demanding C = String points straight at the missing `Resolver<PostTitle, String>` impl for `MyPost`, and rustc even names the `()`-only impl that exists. |
| P2  `p2`  | fails | E0119 | `downstream crates may implement trait \`greem::PartitionTyC<shared::types::SearchResult, _>\` for type \`greem::As<shared::types::User, _>\`` | A C-carrying public entry trait is not knowable-false for `As<User, T>`: a downstream crate may pick its own `C` (a local type in the last trait position), so the blanket overlaps the `As` impl. |
| P3  `p3`  | compiles | – | – | Dropping `C` from the entry trait makes `As<User, T>: PartitionTy<SearchResult>` knowable (local `SearchResult` in the trait, no uncovered param), so the blanket is coherent. But the user impl has no `C` in scope: it must pin the inner completions to one context, and the blanket then hands out `Outputs<SearchResult, C>` for every `C` (`check_string::<Hit>` is accepted although `MyPost` only resolves for `()`). |
| P4  `p4`  | fails | E0119 | `upstream crates may add a new impl of trait \`greem::PartitionNoTag\` for type \`greem::As<shared::types::User, _>\` in future versions` | A tag-less entry trait has no local type in the trait ref at all, so greem itself could implement it for `As`; not knowable. |
| P5  `p5`  | compiles | – | – | `E: PartitionTyC<SearchResult, C> + PartitionTy<SearchResult>`: the knowable-false C-less bound is enough to prove the intersection empty, so the C-carrying blanket becomes coherent. The derive emits both impls for `Hit` (marker + real partition); `Hit`, `As`, `Either` all pass `Outputs<SearchResult, ()>`. |
| P5b `p5b` | fails (only on `check2`) | E0277 | `required for \`Hit<'static>\` to implement \`PartitionTyC<SearchResult, String>\`` then `... to implement \`Outputs<SearchResult, String>\`` | Through the P5 blanket, the context requirement still propagates: C = String fails on the same missing `MyPost: Resolver<PostTitle, String>`, with one extra hop in the note chain. |

## First two lines of each failing diagnostic

P1b:
```
error[E0277]: the trait bound `shared::MyPost: Resolver<PostTitle, String>` is not satisfied
  --> app/examples/p1b.rs:40:14
```

P2:
```
error[E0119]: conflicting implementations of trait `greem::__private::Completes<As<User, _>, _>` for type `SearchResult`
  --> app/examples/p2.rs:6:1
```

P4:
```
error[E0119]: conflicting implementations of trait `greem::__private::Completes<As<User, _>, _>` for type `SearchResult`
  --> app/examples/p4.rs:6:1
```

P5b:
```
error[E0277]: the trait bound `shared::MyPost: Resolver<PostTitle, String>` is not satisfied
  --> app/examples/p5b.rs:48:14
```

## Takeaways

- P1 (derive emits the `Completes` impl directly, with per-variant `Outputs<Ty, C>` where-clauses) is coherent and gives the best error: no extra trait, no marker.
- Any public "partition" entry trait that carries `C` cannot be used as a blanket bound on its own (P2). Removing `C` fixes coherence but loses context propagation (P3); removing the tag breaks coherence outright (P4).
- If a public entry trait is wanted anyway, P5 works: bound on the C-carrying trait *plus* a C-less tagged marker; the marker's knowable-false-ness carries the whole blanket. Costs one extra impl per derive and one extra hop in error notes (P5b).
