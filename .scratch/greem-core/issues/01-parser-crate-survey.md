# Which parser/validator crate does greem reuse?

Type: research
Status: resolved
Blocked by: 
Map: ../map.md

## Question

Survey apollo-compiler / apollo-parser, async-graphql-parser, graphql-parser, cynic-parser (and any other maintained Rust GraphQL parser). For each: maintenance status and release cadence, spec draft tracked, SDL + executable document support, validation coverage (which spec validation rules, whether validation can run against a pre-built schema cheaply per request), error-recovery/lossless-ness, dependency weight and compile-time cost, license. Also: can the same crate serve both build.rs (SDL → schema model for codegen) and runtime (document validation, introspection)? Recommend one, with the concrete API entry points greem-build and the runtime would call.

## Answer

Use **apollo-compiler 1.33.x** (brings apollo-parser) for both `greem-build` and the runtime. It is the only surveyed crate with spec-complete schema *and* executable validation that runs against a pre-built `Valid<Schema>`, plus introspection execution (`introspection::partial_execute`) and variable coercion (`request::coerce_variable_values`); it is also the most actively released (1.33.0 on 2026-09-03, repo pushed 2026-09-21) and its lossless CST + `DiagnosticList` give the best build-step error UX. Cost: 81 crates and ~8 s cold debug / ~14 s release build for an empty binary (vs 3.5 s for cynic-parser), MSRV = latest stable. MIT OR Apache-2.0.
Entry points: build.rs `Schema::parse_and_validate` (or `SchemaBuilder` for multi-file) → walk `schema.types` / `schema_definition` / `implementers_map()`; runtime `ExecutableDocument::parse_and_validate(&schema, ..)` → `operations.get(name)` → `coerce_variable_values` → `partial_execute` for the introspection slice, greem's BFS executor for the rest.
Rejected: async-graphql validation is `pub(crate)` and bound to a derive-built `Registry` (no SDL path); graphql-parser is unmaintained (last release 2024-12) with no validation; cynic-parser (MPL-2.0, tracks 2025 spec) has no validation; graphql-tools is a Hive-internal graphql-parser fork with linear type lookups and no schema model; graphql-query is query-language-only.
Plan for the 2.0 bump (Sept 2025 spec, `@oneOf`; currently beta on the `2026_spec` branch) by keeping apollo types out of generated public API.
Details and per-claim sources: [research/parser-crate-survey.md](../research/parser-crate-survey.md).
