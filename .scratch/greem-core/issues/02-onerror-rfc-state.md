# What exactly is the current onError / error-behavior RFC?

Type: research
Status: resolved
Blocked by: 
Map: ../map.md

## Question

Pin the current state of the GraphQL spec proposal for client-controlled error behavior (variously 'onError', 'error behavior', '@behavior'). Primary sources: graphql/graphql-spec PRs and graphql/graphql-wg RFC docs. Capture: request syntax (where the flag lives: document directive, request field, both), exact mode names and semantics (propagate / no-propagate / halt or equivalents), what HALT means for partially-executed responses and mutations, interaction with non-null positions and with incremental delivery, stage in the RFC process, and which reference implementations (graphql-js, graphql-ruby, others) ship it and how. Quote the normative text where it exists.

## Answer

- The live RFC is graphql-spec #1163 (benjie, RFC 1 "Proposal", open, last touched 2026-09-03); #1236 (martinbonnin, unlabelled) is the same text minus `pathNonNull` and service capabilities. Nothing is merged into the spec, and it missed the Sept/Oct 2026 spec cut. Nullability WG archived 2026-02-05 in favour of this proposal.
- Syntax: a request attribute `onError` (sibling of `query`/`variables`), not a document directive. Values are exactly `"NULL"`, `"PROPAGATE"`, `"HALT"`; anything else is a request error. Default is `"PROPAGATE"` (WG 2026-08-06: "forever"); #1236 alone leaves the default implementation-defined.
- Semantics: the error is always appended to `errors` first (one per response position, now mode-independent). `NULL`: position becomes `null` even if non-null. `PROPAGATE`: classic bubbling, siblings may be cancelled. `HALT`: abort the current `ExecuteRootSelectionSet()` immediately, `data: null`, `errors` = the single first error; subscriptions keep the stream, only the event is dropped. A resolver returning `null` in a non-null position still raises an execution error in every mode.
- Mutations: no special text; under `NULL` later root fields keep executing (confirmed by benjie in review, thread unresolved); under `HALT` earlier side effects stand and later fields "may be cancelled".
- Incremental delivery: unaddressed in both PRs; benjie says the intended unit of `HALT` is the individual incremental result, matching #1110's "incremental result considered failed" rule for propagation.
- #1163 also adds a mandatory `pathNonNull: [bool]` per error (contested); companion #1184 (RFC 2) says stop adding sibling errors after propagation.
- Implementations: graphql-js 17.0.x ships only `@experimental_disableErrorPropagation` (= `NULL`); `onError` PRs #4364/#4842/#4846 are open. graphql-java: directive merged, `onError` PR #4378 open (HALT throws AbortExecutionException with all errors so far). Hot Chocolate parses `onError` but dropped HALT in 2026-03 (enum is Propagate/Null). Apollo Kotlin ships `OnError { NULL, PROPAGATE, HALT }`. graphql-ruby ships nothing.

Full write-up with quoted normative text and sources: [../research/onerror-rfc-state.md](../research/onerror-rfc-state.md)
