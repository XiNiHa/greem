# Parser / validator crate survey

Ticket: [Which parser/validator crate does greem reuse?](https://github.com/XiNiHa/greem/issues/3)
Date: 2026-09-23. Standing decision this serves: "Reuse an existing parser/validator crate for v0" (map.md).

Method: crates.io API for versions/dates/licenses; GitHub/Codeberg APIs for repo activity; crate
sources read from the local cargo registry (`~/.cargo/registry/src/index.crates.io-*/<crate>-<ver>/`,
cited below as `<crate>/<path>`); dependency counts via `cargo tree -e normal --prefix none | sort -u`
on a `fn main(){}` binary depending on exactly one crate; cold build wall-clock on an Apple M3 (8 cores),
rustc 1.98.0, after `cargo fetch`, so the numbers are compile time only, not download.

## Candidates

Versions surveyed (latest stable / latest pre-release on crates.io as of 2026-09-23):

| crate | stable | pre-release | license | repo |
|---|---|---|---|---|
| apollo-compiler | 1.33.0 (2026-09-03) | 2.0.0-beta.1 (2026-08-28) | MIT OR Apache-2.0 | github.com/apollographql/apollo-rs |
| apollo-parser | 0.8.6 (2026-05-14) | 0.9.0-beta.0 (2026-08-21) | MIT OR Apache-2.0 | same |
| async-graphql-parser | 7.2.1 (2026-01-20) | 8.0.0-rc.5 (2026-04-21) | MIT OR Apache-2.0 | github.com/async-graphql/async-graphql |
| graphql-parser | 0.4.1 (2024-12-03) | — | MIT/Apache-2.0 | github.com/graphql-rust/graphql-parser |
| cynic-parser | 0.11.2 (2026-07-12) | — | MPL-2.0 | codeberg.org/obmarg/cynic (GitHub mirror archived 2026-03-07) |
| graphql-tools | 0.5.8 (2026-07-26) | — | MIT | github.com/graphql-hive/router (monorepo) |
| graphql-query | 1.0.1 (2026-03-22) | — | MIT | github.com/StellateHQ/graphql-query |

Source: `https://crates.io/api/v1/crates/<name>` (`max_stable_version`, `versions[].created_at`,
`versions[].license`, `repository`), fetched 2026-09-23. `graphql-ast` does not exist on crates.io.

## Per-crate findings

### apollo-compiler (+ apollo-parser)

**Maintenance / cadence.** Repo pushed 2026-09-21; 73 open issues; 606 stars (GitHub API `repos/apollographql/apollo-rs`).
Releases in the last 13 months: 1.29.0 (2025-08-08), 1.30.0 (2025-08-27), 1.31.0 (2025-11-10), 1.31.1 (2026-02-25),
1.32.0 (2026-05-14), 1.33.0 (2026-09-03), plus 2.0.0-beta.0/1 (2026-08-21/28) (crates.io versions). The 1.x line is
still receiving features in parallel with the 2.0 betas (1.33.0 added `@defer` validation on 2026-09-03;
`apollo-compiler/CHANGELOG.md`). `main` is at 1.33.0; 2.0 work lives on the `2026_spec` branch (GitHub branches list).
MSRV: none declared; README says "tested on the latest stable version of Rust. Older version may or may not be
compatible" (`apollo-compiler/README.md` "Rust versions").

**Spec tracked.** 1.x: October 2021 for parsing and validation (`apollo-compiler/README.md` links
`spec.graphql.org/October2021/#sec-Validation`; `apollo-parser/README.md` "Typed GraphQL Concrete Syntax Tree as per
October 2021 specification"), with draft features back-ported: interface field covariance (`IsValidImplementationFieldType`,
1.32.0), `@defer` validation per graphql-spec#1110 (1.33.0). 2.0.0-beta: "Built-in schema follows the September 2025
specification", `@oneOf` input objects incl. `__Type.isOneOf`, descriptions on executable definitions, September 2025
validation/execution rules (default-value validation, `@deprecated` restrictions), 5.8.5 nested variable-usage fix
(`apollo-compiler-2.0.0-beta.1/CHANGELOG.md`). apollo-parser 0.9.0-beta.0 adds September 2025 string/`SourceCharacter`
grammar and renames `VariableDefinitions` → `VariablesDefinition` (`apollo-parser-0.9.0-beta.0/CHANGELOG.md`).

**SDL + executable.** Both, as separate typed models: `Schema` (`types: IndexMap<NamedType, ExtendedType>`,
`directive_definitions`, `schema_definition`, `sources`) and `ExecutableDocument` (`operations`, `fragments`), plus a
lower-level `ast::Document` for mixed files (`apollo-compiler/src/schema/mod.rs` struct `Schema`; `src/executable/mod.rs`).
Schema can be assembled from multiple files via `SchemaBuilder` / `Parser::parse_into_schema_builder`; executable via
`ExecutableDocumentBuilder` (1.32.0). Every node is `Node<T>` (triomphe Arc) carrying a source span, so diagnostics point
at `file:line:col` in the SDL.

**Validation coverage.** Schema rules and executable rules are separate passes. Executable entry
`validate_executable_document(errors, schema, doc)` runs `validate_operation_definitions`, `validate_fragments_used`,
`validate_defer` (schema-optional) and, with a schema, `validate_subscription` and `FieldsInSetCanMerge` per operation
(`apollo-compiler/src/executable/validation.rs`). The diagnostic enum enumerates what is checked
(`src/validation/diagnostics.rs` `DiagnosticData`): UniqueVariable, UniqueArgument, UniqueInputValue, UndefinedArgument,
UndefinedDefinition, UndefinedDirective, UndefinedVariable, UndefinedFragment, UndefinedEnumValue, UndefinedInputValue,
MissingInterfaceField, InvalidImplementationFieldType, MissingInterfaceFieldArgument,
InvalidImplementationFieldArgumentType, ExtraRequiredImplementationFieldArgument, RequiredArgument, RequiredField,
TransitiveImplementedInterfaces, OutputType, InputType, VariableInputType, QueryRootOperationType, UnusedVariable,
RootOperationObjectType, DuplicateRootOperationType, UnionMemberObjectType, UnsupportedLocation, UnsupportedValueType,
IntCoercionError, FloatCoercionError, UniqueDirective, MissingSubselection, InvalidFragmentTarget, InvalidFragmentSpread,
UnusedFragment, DisallowedVariableUsage, RecursiveDirectiveDefinition, RecursiveInterfaceDefinition,
RecursiveInputObjectDefinition, RecursiveFragmentDefinition, DeeplyNestedType, RecursionError, EmptyFieldSet,
EmptyValueSet, EmptyMemberSet, EmptyInputValueSet, ReservedName. That covers spec §5 (operations, fields incl.
field-selection merging, arguments, fragments, values, directives, variables) and §3 type-system validation. Field merging
uses a typed arena per document (`FieldsInSetCanMerge::new(&alloc, ...)`), i.e. the algorithm from the "Field Selection
Merging" section with caching; there are criterion benches for fields/fragments/directives validation
(`apollo-compiler/Cargo.toml` `[[bench]]`).

**Per-request validation against a pre-built schema.** Yes, this is the primary API shape:
`ExecutableDocument::parse_and_validate(&Valid<Schema>, source, path) -> Result<Valid<ExecutableDocument>, WithErrors<_>>`
takes an already-validated schema by reference and only validates the document (`src/executable/mod.rs`). Validity is a
type-state wrapper `Valid<T>` (`Deref<Target = T>`, `assume_valid`, `into_inner`) so the executor can require
`&Valid<ExecutableDocument>` statically (`src/validation/mod.rs`). Parser limits: `Parser::recursion_limit` (default 500)
and `Parser::token_limit` (`apollo-parser/src/parser/mod.rs` `DEFAULT_RECURSION_LIMIT`, `token_limit`).

**Error recovery / losslessness.** apollo-parser is a rowan-based lossless CST with error resilience: "lexing and
parsing does not fail or `panic` if a lexical or a syntax error is found... `parser.parse()` will always produce a CST"
accompanied by `cst.errors()` (`apollo-parser/README.md`). apollo-compiler collects all errors in a `DiagnosticList`
(ariadne-rendered `Display`, `unstable_to_json_compat()` → GraphQL-response-shaped error) rather than stopping at the
first (`src/validation/mod.rs`).

**Introspection and request plumbing (runtime).** `introspection::partial_execute(&Valid<Schema>, &HashMap<Name,
Implementers>, &Valid<ExecutableDocument>, &Operation, &Valid<JsonMap>) -> Result<ExecutionResponse, RequestError>`
executes only the `__schema`/`__type`/`__typename` portion of an operation (`src/introspection/mod.rs`);
`introspection::check_max_depth` guards against introspection-depth abuse. `request::coerce_variable_values(&Valid<Schema>,
&Operation, &JsonMap) -> Result<Valid<JsonMap>, RequestError>` implements spec CoerceVariableValues (`src/request.rs`).
`Schema::implementers_map()` precomputes interface → {objects, interfaces} once per schema (`src/schema/mod.rs`).
There is also a callback-based reference executor `resolvers::Execution::{execute_sync, execute_async}` over
`ObjectValue`/`AsyncObjectValue` traits (`src/resolvers/mod.rs`) which is depth-first, one field at a time; the 2.0 beta
changelog documents it being audited against graphql-js. Not the execution model greem wants, but a usable oracle for
the BFS≡DFS property tests the map calls for.

**Weight.** 81 unique crates in the normal dep graph (vs 1 baseline). Direct deps (`apollo-compiler/Cargo.toml`, no
`[features]` to trim): ahash, apollo-parser, ariadne (auto-color), futures, indexmap, rowan, serde, serde_json_bytes
(which pulls jsonpath-rust → pest + regex), thiserror, triomphe, typed-arena. Cold build of `fn main(){}` + crate: debug
8.1 s, release 13.8 s (2.0.0-beta.1: 8.1 s / 13.3 s). apollo-parser alone: 16 crates, 2.3 s / 2.7 s.

### async-graphql-parser (+ async-graphql validation)

**Maintenance.** Repo last pushed 2026-04-21 (the 8.0.0-rc.5 bump); 254 open issues; 3682 stars (GitHub API). 7.x got
patch releases through 2026-01-20; the 8.0 line has been in RC since 2026-01-22 with no stable release five months
later (crates.io versions). Parser edition 2024, no `rust-version`; the server crate declares `rust-version = 1.89.0`.

**Spec tracked.** No spec version stated in `async-graphql-parser/src/lib.rs` docs ("A parser for GraphQL. Used in the
async-graphql crate. It uses pest"). The grammar (`src/graphql.pest`) has `repeatable` but no descriptions on
operations/fragments (`fragment_definition = { "fragment" ~ name ~ type_condition ~ directives? ~ selection_set }`),
i.e. pre-September-2025.

**SDL + executable.** Both: `parse_query(&str) -> Result<ExecutableDocument>` and `parse_schema(&str) ->
Result<ServiceDocument>` (`src/parse/executable.rs`, `src/parse/service.rs`). AST is positioned (`Positioned<T>`), owned
Strings, no source map for multi-file.

**Validation.** Not in the parser crate. The server crate's rules (`async-graphql/src/validation/rules/`: 23 files,
graphql-js-style names such as `overlapping_fields_can_be_merged`, `variables_in_allowed_position`, `upload_file`) are
driven by `pub(crate) fn check_rules(registry: &Registry, ...)` (`src/validation/mod.rs`), so they are not callable from
outside async-graphql, and `Registry` is built from Rust types via derive macros — a GitHub code search for
`ServiceDocument` in the repo hits only the parser crate, i.e. there is no SDL → `Registry` path. Reusing validation
would mean vendoring the rules and re-targeting them at greem's own schema model.

**Error recovery.** pest: a single `Error::Syntax { message, start, end }` at the first failure (`src/lib.rs` enum
`Error`); no CST, no recovery.

**Weight.** 24 crates (async-graphql-value, pest, serde, serde_json, indexmap, bytes); debug 4.1 s, release 6.1 s.

### graphql-parser

**Maintenance.** Last commit 2025-01-16 ("Make MSRV explicit"); last release 0.4.1 on 2024-12-03; 8 releases since
2018; open issue #95 "Cut a new release for recent changes?" since 2025-05-18; 24 open issues, several of them
spec gaps or ergonomics (#60 directives on variable definitions, #53 i64 vs i32, #40 spans instead of positions, #80
serde) (GitHub API `repos/graphql-rust/graphql-parser`). 36.9 M downloads / 5.1 M recent, driven by juniper and
transitive users, not by feature work.

**Spec tracked.** Docs describe SDL as "still in RFC" (`graphql-parser/src/lib.rs` crate docs), i.e. 2018-era; the
AST does have `implements_interfaces` on `InterfaceType` and `repeatable` on directive definitions
(`src/schema/ast.rs`, `src/schema/grammar.rs`), so it parses mainstream 2021 SDL.

**SDL + executable.** Both (`query::parse_query`, `schema::parse_schema`), with a formatter; generic over `Text<'a>`.

**Validation.** None. **Error recovery.** combine-based; single `ParseError`. **Weight.** 13 crates (combine, thiserror);
debug 2.7 s, release 3.2 s. Edition 2018.

### cynic-parser

**Maintenance.** Development moved to Codeberg; the GitHub repo is archived (GitHub API `repos/obmarg/cynic`
`archived: true`, pushed 2026-03-07). Codeberg: last commit 2026-07-18, 60 open issues (Codeberg API
`repos/obmarg/cynic`). Releases: 0.9.0 (2025-02), 0.9.1 (2025-02), 0.10.0 (2025-08), 0.11.0 (2026-02), 0.11.1
(2026-04), 0.11.2 (2026-07) (crates.io). MSRV 1.85 (`cynic-parser/Cargo.toml`). Single primary maintainer (obmarg);
primarily serves the `cynic` client and Grafbase.

**Spec tracked.** README still says "compatible with the 2021 GraphQL specification or earlier"
(`cynic-parser/README.md`), but the 0.11.0 changelog says "Added support for most of the GraphQL 2025 spec": schema
coordinates, descriptions in executable documents, `InputObjectDefinition::is_one_of`
(`cynic-parser/CHANGELOG.md`, fetched from Codeberg).

**SDL + executable.** Both: `parse_type_system_document(&str) -> Result<TypeSystemDocument, Error>` and
`parse_executable_document(&str) -> Result<ExecutableDocument, Error>` (`src/lib.rs`). Arena/ID-based readers
(`src/type_system/ids.rs`, `src/executable/ids.rs`), designed for low memory and compile speed (README "Design Goals").
Optional features: `report` (ariadne error reports), `print`, `pretty` (`Cargo.toml` `[features]`).

**Validation.** None (no validation module in `src/`; no `validat` hits outside error text).
**Error recovery.** lalrpop + logos; neither `schema.lalrpop` nor `executable.lalrpop` uses lalrpop's `!` recovery
production, so parsing stops at the first `Error::{InvalidToken, UnrecognizedEof, UnrecognizedToken, ...}`
(`src/errors.rs`). **Weight.** 20 crates (23 with `report`+`print`): lalrpop-util, logos, indexmap; debug 3.5 s,
release 4.4 s. **License** MPL-2.0 (file-level copyleft; fine to depend on, but the odd one out in this list).

### graphql-tools (The Guild / Hive)

**Maintenance.** Now lives in the `graphql-hive/router` monorepo (`lib/graphql-tools`); pushed 2026-09-23; releases
roughly monthly (0.5.1 2026-02-08 … 0.5.8 2026-07-26) (crates.io, GitHub API). It exists to serve Hive Router.

**What it is.** A vendored fork of graphql-parser (`graphql-tools/src/parser/mod.rs` reproduces the graphql-parser crate
docs; `src/lib.rs`: "Most of the tools are based on traits and structs implemented in graphql_parser crate") plus a
validation layer with 27 graphql-js-named rules (`src/validation/rules/`: fields_on_correct_type,
overlapping_fields_can_be_merged, values_of_correct_type, variables_in_allowed_position, single_field_subscriptions,
unique_directives_per_location, ...) and an `introspection` module (schema from introspection JSON).

**Per-request validation.** `validate(schema: &schema::Document, operation: &query::Document, plan: &ValidationPlan) ->
Vec<ValidationError>` (`src/validation/validate.rs`) runs against the raw schema AST; type lookup is a linear scan over
`definitions` (`src/ast/ext.rs` `type_by_name`), so per-request cost grows with schema size unless greem builds its own
index. No schema validation, no introspection execution.

**Weight.** 31 crates (combine, serde_with + darling, xxhash-rust); debug 5.8 s, release 7.9 s. MIT.

### graphql-query (Stellate)

Query-language only by design: "does not aim to support full, server-side GraphQL execution or the GraphQL Schema
Language" (`graphql-query/README.md`). 10 schema-less validation rules (`src/validate/rules/`: known_fragment_names,
lone_anonymous_operation, no_fragment_cycles, no_undefined_variables, no_unused_fragments, no_unused_variables,
unique_argument_names, unique_fragment_names, unique_operation_names, unique_variable_names); README lists
"Schema-aware validation rules" as not done. Two releases ever (2024-05-21, 2026-03-22); 99 stars. 39 crates (bumpalo,
logos, lexical-core, hashbrown); debug 4.7 s, release 5.7 s. MIT, MSRV 1.71.1. Not a fit for a server.

## Comparison

| | apollo-compiler 1.33 | async-graphql-parser 7.2 | graphql-parser 0.4 | cynic-parser 0.11 | graphql-tools 0.5 |
|---|---|---|---|---|---|
| Last release / commit | 2026-09-03 / 2026-09-21 | 2026-01-20 (rc 2026-04-21) / 2026-04-21 | 2024-12-03 / 2025-01-16 | 2026-07-12 / 2026-07-18 | 2026-07-26 / 2026-09-23 |
| Spec | Oct 2021 (+ 2025 bits); 2.0-beta = Sep 2025 | unstated, pre-2025 grammar | 2018-era + `repeatable`, iface-implements | "most of 2025" | 2021-era (graphql-parser fork) |
| SDL / executable | both, typed models + AST | both | both | both | both |
| Schema validation | yes | no | no | no | no |
| Executable validation | yes, ~full §5, against `&Valid<Schema>` | `pub(crate)`, needs `Registry` | no | no | 27 rules, against schema AST (linear lookups) |
| Introspection exec | yes (`partial_execute`) | no | no | no | no |
| Variable coercion | yes | no | no | no | no |
| Error recovery | lossless CST, all errors | first error | first error | first error | first error |
| Unique deps | 81 | 24 | 13 | 20 | 31 |
| Cold debug / release | 8.1 s / 13.8 s | 4.1 s / 6.1 s | 2.7 s / 3.2 s | 3.5 s / 4.4 s | 5.8 s / 7.9 s |
| License | MIT OR Apache-2.0 | MIT OR Apache-2.0 | MIT/Apache-2.0 | MPL-2.0 | MIT |
| MSRV | latest stable (undeclared) | none (server: 1.89) | none | 1.85 | none |

## Can one crate serve both build.rs and runtime?

Only apollo-compiler covers both ends without greem writing a validator:

- **build.rs** needs SDL → a resolved schema model with types, fields, arguments, interfaces/unions, directives, source
  spans for error reporting, and schema validation so codegen never sees an invalid schema. `Schema::parse_and_validate`
  / `SchemaBuilder` gives exactly that; `DiagnosticList`'s `Display` renders ariadne reports pointing at the `.graphql`
  file, which is what a `build.rs` should print on failure.
- **runtime** needs, per request: parse + validate an operation against the same schema, coerce variables, answer
  introspection, and expose a typed operation tree (fields, response keys, type conditions, fragments) for lookbehind
  planning. `ExecutableDocument::parse_and_validate(&Valid<Schema>, ...)`, `request::coerce_variable_values`,
  `introspection::partial_execute`, `Operation`/`SelectionSet`/`Field::response_key` provide those.

The other crates would each require greem to implement schema validation, executable validation and introspection
itself for v0 (cynic-parser, graphql-parser, async-graphql-parser), or to adopt a Hive-internal fork with no schema
model and linear type lookups (graphql-tools). That is the opposite of the standing decision to reuse.

## Recommendation

Adopt **apollo-compiler 1.33.x** (which brings apollo-parser) as greem's single parser + validator for both
`greem-build` and the runtime. Reasons: it is the only candidate with spec-complete schema and executable validation
that runs against a pre-built `Valid<Schema>`, it ships introspection execution and variable coercion greem would
otherwise have to write, it is the most actively released crate in the set, its diagnostics are the best for a
build-step UX, and the license matches the others. The price is dependency weight (81 crates, ~7.5 s extra cold debug
build, +13 s release) and an undeclared MSRV; accept it for v0 and revisit only if a `[features]` split lands upstream
(none exists today) or the weight shows up as a user complaint.

Concrete entry points:

- `greem-build` (build.rs):
  - `apollo_compiler::Schema::parse_and_validate(sdl, path)` → `Valid<Schema>`; for multi-file schemas
    `Schema::builder()` + `Parser::new().parse_into_schema_builder(...)` then `.build()` and `.validate()`.
  - On `Err(WithErrors { errors, .. })`, print `errors` (Display) and fail the build.
  - Walk `schema.types: IndexMap<NamedType, ExtendedType>` (`Object`/`Interface`/`Union`/`Enum`/`InputObject`/`Scalar`),
    `schema.schema_definition` for root operation types, `schema.implementers_map()` for abstract-type dispatch tables;
    emit Rust. Keep apollo types out of the generated public API so the 2.0 bump stays internal.
  - Embed the SDL (`include_str!`-style via `OUT_DIR`) so the runtime re-parses the identical text.
- runtime (once per process): `Schema::parse_and_validate(embedded_sdl, "schema.graphql")` → `Valid<Schema>` plus a
  cached `implementers_map()`. Build-time validation makes this a cannot-fail path in practice; keep it a `Result`.
- runtime (per request):
  - `ExecutableDocument::parse_and_validate(&schema, query, "request.graphql")` → `Valid<ExecutableDocument>`; on error,
    `DiagnosticList::iter()` + `Diagnostic::unstable_to_json_compat()` to produce spec-shaped `errors`.
  - `doc.operations.get(operation_name)` → `&Operation`; `request::coerce_variable_values(&schema, op, &vars)` →
    `Valid<JsonMap>`.
  - `introspection::check_max_depth(&doc, op)` then `introspection::partial_execute(&schema, &implementers, &doc, op,
    &coerced_vars)` for the `__schema`/`__type` slice; greem's BFS executor handles the rest, reading
    `Operation::selection_set`, `Field::response_key()`, `Field::definition`, `InlineFragment::type_condition`,
    `FragmentSpread` → `doc.fragments`.
  - Configure `Parser::new().recursion_limit(..).token_limit(..)` for request parsing rather than the `Default`.
- compliance tests: `resolvers::Execution::new(&schema, &doc).execute_sync(...)` is a spec-audited depth-first executor
  that can serve as an external oracle alongside greem's own naive reference executor.

Version policy: pin `apollo-compiler = "1"` now; track `2.0.0-beta.*` on the `2026_spec` branch and bump when 2.0
stabilises. Known migration items from the beta changelog: `Component<T>` → `Node<T>`, `response_key()` →
`response_name()`, `resolvers::FieldError` → `ExecutionError`, `DirectiveDefinition.locations` becomes an `IndexSet`,
new `description` fields on executable definitions, `@oneOf` built in. If greem's custom scalars ticket wants
`@oneOf`/September-2025 semantics before then, switching to the beta is the cheaper path than patching 1.x.

Runner-up if weight ever becomes disqualifying: cynic-parser (20 crates, 3.5 s, actively tracking the 2025 spec) as
the parser, with greem owning validation and introspection itself, accepting MPL-2.0 in the tree.
