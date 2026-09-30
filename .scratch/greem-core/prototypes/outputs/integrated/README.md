# THROWAWAY: integrated Outputs + breadth-first ownership probe

Run from the repository root:

```sh
cargo run --offline --manifest-path .scratch/greem-core/prototypes/outputs/integrated/Cargo.toml
```

The cached dependencies are futures, serde, and serde_json. This fixture compiles/runs on Rust 1.98.0 with no feature flags, unsafe, runtime spawning, object Any, or application type mapping. The runtime is a library crate; the manually generated schema and application are the separate binary crate. It is a type/lifetime prototype, not the walking skeleton or a GraphQL server.

## What runs together

- One generic `Outputs` bridge into generated object-tag `Completes` impls, with real methods.
- Query → User → Post → User execution over mutually recursive resolver requirements.
- Owned User names, Post outputs borrowing User names and request/context data, and borrowed scalar outputs.
- Nested lists, nullable positions and list levels, per-position object errors, and abstract partition through `Either` plus a sub-interface wrapper.
- The accepted reserved parent-error hook filters failed objects before any child scope is created.
- A hand-expanded per-object name adapter joins object futures, alongside a manual set-based posts resolver. It is generic in C beside concrete App-context impls.
- Scope-erased, Send futures; generated code statically joins each scope's field futures, then the executor joins all scopes before entering the next generation.
- A deliberately pending posts future proves both User scopes start before either completes, and no Post author resolver starts until both parent scopes finish.
- Owned output batches remain in nested generation frames. Batches require Send, not Sync; projected scope objects must satisfy the Resolver Send + Sync bound.
- Fixed OnceLock slots lazily hold codegen Plan values through `Box<dyn Any + Send + Sync>`. Generated code borrows their arguments. The object values do not use Any.
- A borrowing `Response: Serialize` is consumed by a higher-ranked callback while every referenced owner is still alive. Only the serialized String escapes; Post destructors run afterward and read their live parent strings.
- Cancelling while Post resolution is pending drops all three Post values safely, and never calls the serialization callback.

## Two contract choices accepted after integration

**Partition identity:** the two abstract arms ultimately contain the same `Result<User, Error>` Rust type and GraphQL tag, but produce separate scopes of two and one successful objects. The generic implementation does not pretend to establish type equality across arms. Proposed identity includes parent scope, field tree position and representation partition leaf; one resolver invocation per nonempty scope. Iha Shin accepted this contract after reviewing the separate [partition probes](../partition/README.md).

**Response API:** the primitive is a synchronous callback, conceptually `execute_with(..., for<'r> FnOnce(Response<'r>) -> R) -> R`. It runs at the deepest retained generation; `R` cannot borrow the temporary response. A convenience API can return owned serialized bytes/text using serde, without first copying every leaf into an owned response arena. Iha Shin accepted this response boundary. The companion [response-boundary probe](../arena/src/bin/response_boundary.rs) also captures the compiler rejection when a callback tries to return a response borrow.

## Diagnostics and timing

`run-output.txt` records the serialized completion trace, generation trace and successful normal/cancellation destruction checks. `timings.txt` records warm local Cargo invocations, not cold build or schema-scaling benchmarks.

```sh
cargo rustc --offline --manifest-path .scratch/greem-core/prototypes/outputs/integrated/Cargo.toml --bin greem-integrated-probe -- --cfg missing
cargo rustc --offline --manifest-path .scratch/greem-core/prototypes/outputs/integrated/Cargo.toml --bin greem-integrated-probe -- --cfg wrong
```

Both commands deliberately fail. Removing the Post author resolver points to `Vec<Post<'a>>` in the referring output bound and identifies the missing field trait. Changing name's output to i32 fails at that associated-type definition. Earlier [type probes](../README.md) separately show boundary rejection and the limitations of `on_unimplemented`: its custom wording is not guaranteed to be rustc's primary message.

## Deliberate limits

The serialized response is an inspectable path/slot trace, **not a GraphQL response or the production columnar result arena**. It checks the serializer lifetime, not GraphQL response formatting, null propagation, error paths, mutation semantics, lookbehind planning, SDL parsing, codegen generation or proc-macro expansion. The name adapter and tag impls are hand-written equivalents of generated code.

The fixture assumes correct output cardinality; it uses assertions/unwrap for prototype failures. The framework's length-mismatch and outer-error handling are not implemented here. Plan slots are fixed for the probe's finite tree; dynamically growing abstract execution trees need stable slot ownership in the real implementation. Alias/tree-node grouping and a complete result-arena implementation belong to the skeleton.

Nested frame polling/destruction has O(generation depth) call chains; this probe does not establish a safe maximum depth. The implementation must bound depth or supply a separately validated strategy before accepting unbounded operations. Ordinary cancellation is demonstrated; panic/unwind exhaustiveness is not established by this fixture.

The original std-Allocator/bump proposal is not exercised or required by this ownership shape. The existing minimum-version policy is not silently changed by successful compilation on 1.98.
