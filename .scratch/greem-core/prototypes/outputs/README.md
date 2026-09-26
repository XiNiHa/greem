# THROWAWAY: Outputs encoding compiler probes

Question: does the per-field Resolver / generated Completes / Outputs bridge survive breadth-first execution, borrowing and downstream-crate coherence?

Run from the repository root:

```sh
python3 .scratch/greem-core/prototypes/outputs/run.py
```

This is a deliberately incomplete prototype, not greem's runtime or walking skeleton. The compiler is the experiment: several cases are supposed to fail. `results/summary.txt` records wall time and exit code; each case has its full diagnostic alongside it. Timing is one local rustc invocation (including linking for successful binaries), not a scaling benchmark or cold Cargo build.

## Initial compiler findings

| Probe | Observed result | Meaning |
|---|---|---|
| `types` | Compiles in a separate downstream crate | Bridge, mutually recursive object bounds, nested lists and nullable values, a generic output, a generic-in-context resolver beside concrete-context resolvers, abstract members/sub-interface delegation and placement of a user-enum completion impl are coherent in isolation. |
| `missing` | Fails with E0277 | Missing field is rejected at the schema boundary and the referring output. The custom Resolver diagnostic is visible on one nested obligation but is not consistently the top-level error. |
| `wrong` | Fails with E0277 | An integer output for a text field is rejected at the associated-type definition. |
| `coherence` | Fails with E0119 | The global per-object sugar blanket overlaps with both reference and Result delegation. |
| `sugar_with_manual` | Fails with E0119 | Even without wrapper delegation, the sugar blanket prevents the example's context-generic manual resolver: a downstream crate may implement ObjectResolver using its own context. |
| `adapter` | Compiles | Replacing the global sugar blanket with explicit per-field bridges permits sugar, a manual set-based field, and both delegation bounds to coexist. |
| `reference_original` | Fails with E0597 | A temporary reborrowed parent slice cannot live for the request. |
| `reference_split` | Compiles | Separate `&'call [&'req Self]`, returning `Output<'req>` in a future bounded by `'call`, permits reference forwarding without extending a temporary borrow. |
| `scope_loop` | Fails with E0597/E0506 | A loop cannot replace owned scopes after borrowing each scope for `'req`. |
| `owner_borrow` | Fails with E0505/E0515 | Moving a Box and references borrowed from its contents into the same returned structure is not a safe-Rust ownership shape by itself. |
| `plan_cache` | Fails with E0499 | An ordinary mutable HashMap cannot lazily insert another Plan while an earlier entry remains borrowed for the request. Stable append-only storage needs a concrete design. |

The trait declaration also needed `C: 'a` on the Output GAT; generated field markers are `'static` in this probe. Neither restriction forces application objects or contexts to be `'static`.

## What these probes do not prove

- `Completes` and `Outputs` in the first probe have no execution methods. The abstract impls prove coherence and recursive bounds, not executable partition or complete arena integration.
- Reference/Result delegation in `runtime.rs` has intentionally unimplemented bodies. `reference.rs` separately checks a real reference-forwarding body and the necessary lifetime split. A bound-only Result impl with an unreachable resolver body is not an executable error-completion design.
- The adapter probe checks trait placement and borrowed output types. Its tiny body is sequential and uses simplified resolver error/context/argument signatures; real sugar must join futures and preserve per-position errors.
- The user enum completion impl proves downstream placement, not a derive implementation or enum partition behavior.
- No SDL codegen, serializer, null propagation or GraphQL validation is implemented.
- These isolated compile results are not a memory-safety proof or full executor proof. The later integrated experiment and live decisions, described below, supply the resolution evidence.

## Executable completion follow-up

After accepting explicit per-field adapters, the [Result completion probe](completion/README.md) demonstrates a candidate with real completion and delegation bodies. It uses a proposed default `Resolver::parent_error` hook through a fixed generated field witness, completes object errors before scheduling children, and retains original successful indices. Iha Shin accepted this hook. The candidate itself is a narrow probe; the subsequent integrated probe below combines it with scopes.

## Ownership experiments

See [arena probes](arena/README.md) for the runnable companion experiments. A fixed table of OnceLock slots supports borrowed, lazily initialized Plans. Safe nested generation frames execute joined Send futures with parent-borrowed objects and child-before-parent Drop, but change the lifetime/ownership seam and require serialization before those frames unwind (or separately owned results). The direct `Box<[T], &Bump>` proposal is not Send because Bump is not Sync. These are findings for the ownership discussion, not an adopted replacement design.

## First decision — agreed

Keep the per-field Resolver primitive. Generate the per-object adapter explicitly for each field, instead of installing one open-ended ObjectResolver-to-Resolver blanket. Retain a macro-free route through explicit Resolver implementations (an explicit adapter macro is another possible convenience).

Iha Shin accepted this direction in the live discussion. The subsequent completion and ownership decisions were accepted and are recorded in the resolved ticket.

## Integrated follow-up

The [integrated prototype](integrated/README.md) combines the accepted adapter/error-hook choices with safe generation-frame ownership, actual nested/sub-interface partition, concurrent field/scope joins, borrowed serialization, and cancellation. Iha Shin accepted its repeated-arm scope identity and callback response API. The ticket is resolved; stable storage for dynamically discovered Plans and the depth-limit policy are separate follow-up decisions.
