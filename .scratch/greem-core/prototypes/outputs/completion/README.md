# THROWAWAY: executable Result completion

Run:

```sh
cargo run --offline --manifest-path .scratch/greem-core/prototypes/outputs/completion/Cargo.toml
```

Observed on rustc 1.98.0. The runtime is in the library crate; hand-written generated schema and user implementations are in the separate binary crate. This preserves the relevant cross-crate coherence boundary.

## Candidate mechanism (awaiting a decision)

`Resolver` has a default `parent_error(&self) -> Option<&Error>` returning None. The runtime's reference delegation forwards it; Result delegation returns its own Err or forwards through Ok. Generated object completion chooses one fixed schema field as its witness and calls that fully-qualified method before creating child scopes. The witness does not depend on selected fields; therefore this catches an erroneous object even when only __typename is selected.

Only successful object positions become scope parents. The real Result resolver delegation then collects references to Ok values and calls the underlying resolver. It has no unreachable body; an erroneous direct call returns an outer error. Runtime scalar/list/nullable tags and generated abstract tags each have disjoint Result completion impls. There is no blanket completion impl over every tag.

The API cost is explicit: object completion depends on a reserved default method of one field resolver. Ordinary user impls need no extra method, but custom overrides could make failure behavior depend on the generated witness. This is intended as a framework hook, not field-specific user validation. If adopted, codegen and docs must specify that restriction and choose a deterministic witness; the probe fixes `name` for User and `author` for Post.

## Executed scenarios

- Three object outputs, one erroneous: the error is completed immediately, without calling child resolvers; successful original positions `[0, 2]` resolve together with one invocation.
- The same precompletion works when no user field is selected (the __typename-only behavior).
- Names borrow from parent objects after the temporary parent-reference slice has been dropped. The signature uses `&'call [&'obj Self]`, `Output<'obj>`, future `+ 'call`, `'obj: 'call`.
- Nullable nested lists containing borrowed objects and per-item errors.
- Nested Result wrappers around an object; Result around an abstract output; an erroneous abstract output.
- A direct call to Result delegation with an erroneous parent returns an error, without a panic or unreachable body.
- Mutually recursive User/Post requirements, borrowed object output, and a context-generic name resolver compile with the completion methods present.

## Limits

The `Value` enum is an inspectable, temporary completion trace, not the planned columnar result arena. Object values record a tag; the probe manually extracts successful parent positions and calls one field. It is not an integrated generic scope builder, executable abstract partition, breadth-first engine, post-hoc null propagation, or serializer. Error paths and ids are not implemented. The full executor still needs its ownership decision and integration proof.

The earlier simplified probes remain intact to preserve their original coherence failures and diagnostics. They are not the proposed implementation.
