# `onError` / error-behavior RFC: state as of 2026-09-23

Ticket: [What exactly is the current onError / error-behavior RFC?](https://github.com/XiNiHa/greem/issues/4)

All claims below are taken from primary sources (graphql-spec PRs, graphql-wg / nullability-wg repos, implementation PRs and source). Quoted spec text is from the PR diffs as fetched on 2026-09-23; nothing here is merged into the published GraphQL specification yet.

## 1. Where the proposal lives

| Artifact | State | Notes |
| --- | --- | --- |
| [graphql-spec #1163](https://github.com/graphql/graphql-spec/pull/1163) "Error behaviors (including `onError: "NULL"`)" (benjie) | **Open, label `💡 Proposal (RFC 1)`**, last updated 2026-09-03 | The canonical RFC. Opened 2025-04-30 as a rewrite of #1153. On 2026-08-03 service capabilities were split out into #1208 and `pathNonNull` was added; on 2026-08-06 the "default error behavior" concept was removed. Diff now touches only `Section 6 -- Execution.md` and `Section 7 -- Response.md`. |
| [graphql-spec #1236](https://github.com/graphql/graphql-spec/pull/1236) "`onError` request parameter" (martinbonnin) | Open, **no stage label**, opened 2026-07-22 | Described as "#1163 with just `onError`, uncoupled from service capabilities". Same three modes and the same normative text for the modes; differs in that `onError` is *optional*, the service's *default error behavior* is "implementation-defined", and there is no `pathNonNull`. Links merged implementations in Apollo Kotlin and Hot Chocolate and open ones in graphql-java and graphql-js. |
| [graphql-spec #1208](https://github.com/graphql/graphql-spec/pull/1208) "Service capabilities" | Open, RFC 1 | Split from #1163. Would carry an optional `graphql.onError` capability that "indicates that this service accepts and honors the `onError` request property" (#1163 body). |
| [graphql-spec #1184](https://github.com/graphql/graphql-spec/pull/1184) "Sibling errors should not be added after propagation" | Open, `📄 Draft (RFC 2)` + `🚀 Next Stage?` | Companion: recommends servers stop appending sibling errors after propagation has occurred, so a client can trust the *last* error at a path. |
| [graphql-spec #1165](https://github.com/graphql/graphql-spec/pull/1165) "Transitional Non-Null appendix (`@noPropagate`)" | Open, `💭 Strawman (RFC 0)`, `💤 stale ?` | Schema-side companion (semantic nullability). Not part of `onError`. |
| Superseded: [#1050](https://github.com/graphql/graphql-spec/pull/1050) (`@disableNullPropagation` operation directive, closed 2026-09-10), [#1153](https://github.com/graphql/graphql-spec/pull/1153) (`onError: "NO_PROPAGATE"`/`"ABORT"`, closed 2025-04-30), #1145, [#895](https://github.com/graphql/graphql-spec/pull/895) (Client Controlled Nullability, closed) | closed | Both #1050 and #1153 carry the `🌱 Superseded (RFC X)` label. |
| graphql-wg RFC docs | informal | [`rfcs/DisableErrorPropagationDirective.md`](https://github.com/graphql/graphql-wg/blob/main/rfcs/DisableErrorPropagationDirective.md) (the earlier directive proposal; already lists `PROPAGATE`/`NULL`/`ABORT` as a "future extension"), [`rfcs/SemanticNullability.md`](https://github.com/graphql/graphql-wg/blob/main/rfcs/SemanticNullability.md) (assumes "clients may opt out of error propagation via some mechanism that is outside the scope of this RFC ... e.g. via a directive such as `@noErrorPropagation` or `@behavior(onError: NULL)`; or via a request-level flag"). The RFC dir README says documents there "imply no specific approval or support". |
| [graphql/nullability-wg](https://github.com/graphql/nullability-wg) | **archived 2026-02-05** | README: "Many thanks to everyone who contributed ... that made it possible to land on the current **service capabilities** + `onError` proposal. Moving forward, the implementation will be carried in the main working group." |

**Stage.** RFC 1 (Proposal) per the label on #1163 and the WG agenda for 2026-01-15 ("Current status: RFC1 via original Error behaviors/service capabilities PR"). Per [graphql-spec CONTRIBUTING.md](https://github.com/graphql/graphql-spec/blob/main/CONTRIBUTING.md), Stage 2 (Draft) requires "working group consensus the problem identified should be solved, and this particular solution is preferred", and Stage 3 (Accepted) "must be implemented in GraphQL.js". Neither has happened: the graphql-js PRs are unmerged (section 6). The proposal did not make the "spec freeze" for the September/October 2026 spec cut discussed at the 2026-07-02 and 2026-09-03 WG meetings (neither summary lists it among the items being frozen).

## 2. Request syntax: where the flag lives

**A request attribute, not a document directive.** Both live PRs add `{onError}` to the list of things "A GraphQL service generates a response from a request via" in Section 6, and thread it through `ExecuteRequest`.

#1163 (current text):

> - {onError} (recommended): The _error behavior_ to apply to the request; see [Handling Execution Errors](#sec-Handling-Execution-Errors). Clients should provide {onError} as part of a GraphQL request. If {onError} is provided and its value is not one of {"NULL"}, {"PROPAGATE"}, or {"HALT"}, then a _request error_ must be raised. If {onError} is not provided, the value {"PROPAGATE"} will be used.
>
> Note: Previous versions of this specification did not define the {onError} request attribute. Clients should only include {onError} in the request if it is known that the service supports this property.

#1236 (current text):

> - {onError} (optional): The _error behavior_ to apply to the request; see [Handling Execution Errors](#sec-Handling-Execution-Errors).
>
> ExecuteRequest(schema, document, operationName, variableValues, onError, initialValue):
> ...
> - If {onError} is not one of {"NULL"}, {"PROPAGATE"}, or {"HALT"}, raise a _request error_.
>
> Note: Detecting whether a service supports {onError} and what _default error behavior_ it uses is outside the scope of this specification.

Over HTTP it is a sibling of `query`/`variables` in the JSON body (from the GraphQL Foundation blog draft by Benjie Gillam, [`2026-08-14-true-nullability.mdx` on branch `nullability-post`](https://github.com/graphql/graphql.github.io/blob/nullability-post/src/pages/blog/2026-08-14-true-nullability.mdx), not yet published on the site's default branch):

```json
{ "query": "...", "onError": "NULL", "variables": { "id": "27" } }
```

The GraphQL-over-HTTP spec (`spec/GraphQLOverHTTP.md` on main) does not mention `onError` yet.

**History of where the flag lived**, for context:

- #1050 (2023): operation directive `@disableNullPropagation` (name "open to workshopping").
- graphql-js #4348 / graphql-java #3772 (early 2025): shipped `@experimental_disableErrorPropagation on QUERY | MUTATION | SUBSCRIPTION` as an interim directive (see section 6).
- WG 2025-04-03 summary: the nullability WG "agreed on Benjie's request parameter solution, which is considered cleaner than the previous query directive approach. This solution allows for potentially defaulting the option to true in future schema versions."
- #1153 → #1163 (2025): request parameter `onError`, plus a *schema*-side default via `directive @behavior(onError: __ErrorBehavior! = PROPAGATE) on SCHEMA` and `__Schema.defaultErrorBehavior` introspection (#1153/#1163 as of 2025-04-30, per graphql-js #4364 comment and review threads), later replaced by a `graphql.defaultErrorBehavior(...)` *service capability*.
- 2026-08-06: the default-behavior concept was dropped entirely (see section 3). The blog draft explicitly rejects the directive approach: "we quickly realised that this was cumbersome and inconsistent, and that disabling error propagation would also become the responsibility of the developer rather than the client".
- A reviewer asked for a directive instead ("What about onError (or behavior) as a query directive rather than a new field on the request? Adding a field to the request payload might require changes to lots of GraphQL middleware", fotoetienne, #1163 review 2025-04-30); the PR kept the request attribute.

## 3. Mode names and semantics

Exact names: **`"NULL"`, `"PROPAGATE"`, `"HALT"`** (strings). Renames along the way: `NO_PROPAGATE` → `NULL` on 2025-07-10 (#1163 comment: "have changed `NO_PROPAGATE` to `NULL`"), `ABORT` → `HALT` (present in #1153, renamed by the time of the current #1163 text; graphql-js #4364 still uses `ABORT`). Any other value is a *request error* (both PRs).

**Default.** #1163: `"PROPAGATE"` when omitted, forever. From the #1163 body: "Per the working group on 2026-08-06 we will forever have PROPAGATE as the default error behavior, so this capability has been removed. But worry not! The proposal is that the `onError` value to use can be incorporated into the trusted documents to save you precious network bytes." The 2026-08-06 WG summary records "Pascal emphasizing the importance of not changing defaults once implemented. The group agreed that new clients like Urkel [sic: urql] and Apollo should default to using on-error null when supported by the server, while maintaining backward compatibility for older clients." #1236 instead says "The _default error behavior_ of a service is implementation-defined" and "Note: {"HALT"} is not recommended as the _default error behavior_".

**Normative core (identical in #1163 and #1236), Section 6 "Errors and Non-Null Types":**

> If during {ExecuteCollectedFields()} a _response position_ with a non-null type raises an _execution error_, the error must be added to the {"errors"} list in the _execution result_ and then handled according to the _error behavior_ of the request:
>
> - {"NULL"}: The _response position_ must be set to {null}, even if such position is indicated by the schema to be non-nullable. (The client is responsible for interpreting this {null} in conjunction with the {"errors"} list to distinguish error results from intentional {null} values.)
> - {"PROPAGATE"}: The _execution error_ must propagate to the parent _response position_ (the entire selection set in the case of a field, or the entire list in the case of a list position). The parent position resolves to {null} if allowed, or else the error is further propagated to a parent response position. Any sibling response positions that have not yet executed or have not yet yielded a value may be cancelled to avoid unnecessary work.
> - {"HALT"}: The current {ExecuteRootSelectionSet()} must be aborted immediately and must yield an execution result with an {"errors"} list consisting of this _execution error_ only and the {"data"} entry set to {null}. Any _response position_ that has not yet executed or has not yet yielded a value may be cancelled to avoid unnecessary work. (Note: For a subscription operation the underlying stream is not terminated.)

**Section 6 "Handling Execution Errors" (both PRs):**

> An _execution error_ is an error raised during field execution, value resolution or coercion, at a specific _response position_. These errors must be added to the {"errors"} list in the _response_, and are "handled" according to the _error behavior_ of the request.
>
> If a _response position_ resolves to {null} because of an execution error which has already been added to the {"errors"} list in the _execution result_, the {"errors"} list must not be further affected. That is, only one error should be added to the errors list per _response position_.
>
> :: The _error behavior_ of a request indicates how an _execution error_ is handled; valid values are {"NULL"}, {"PROPAGATE"} and {"HALT"}. The _error behavior_ for a _request_ should be specified using the {onError} attribute of the request; if unspecified, the _error behavior_ is {"PROPAGATE"}. *(#1163 wording; #1236: "It may be specified using the optional {onError} attribute of the _request_. If omitted, the _default error behavior_ of the service applies.")*
>
> Regardless of error behavior, if a _response position_ with a non-null type results in {null} due to the result of {ResolveFieldValue()} then an execution error must be raised at that position as specified in {CompleteValue()}.
>
> The _error behavior_ of a request applies to every _execution error_ raised during execution.
>
> **{"NULL"}**
>
> With {"NULL"}, a `Non-Null` _response position_ will have the value {null} if and only if an error occurred at that position.
>
> Note: Clients must inspect the {"errors"} list and use the {"path"} of each error result to distinguish between intentional {null} values and those resulting from an _execution error_.
>
> **{"PROPAGATE"}**
>
> With {"PROPAGATE"}, a `Non-Null` _response position_ must not contain {null} in the _response_.
>
> [existing propagation text retained: nearest nullable ancestor, list positions, `data: null` if every position to the root is Non-Null]
>
> **{"HALT"}**
>
> With {"HALT"}, {ExecuteRootSelectionSet()} must cease immediately that the first _execution error_ is raised. That error must be added to the {"errors"} list, and {"data"} must be {null}.
>
> Note: For subscription operations, processing of the current event is ceased, but the subscription still remains in place and future events will be processed as normal.

Takeaways for an executor:

- The error is **always** recorded in `errors` first; the mode only decides what happens to `data`. The "one error per response position" rule is now mode-independent.
- `NULL` does *not* disable the non-null check on a resolver returning `null` — that still raises an execution error at that position; the mode then decides that the position becomes `null` instead of propagating.
- `HALT` is defined at the granularity of `ExecuteRootSelectionSet()` (one operation execution / one subscription event), yields `data: null`, and `errors` contains **exactly one** error (the first). This is the strongest statement of "first error wins": whichever error is observed first under the implementation's scheduling is the one reported.

### `pathNonNull` (#1163 only, added 2026-08-03)

Section 7 gains a *response path nullability* definition and a required `pathNonNull` entry on every execution error:

> :: The _response path nullability_ of a _response path_ is a list of boolean values having the same length as the response path. Each value corresponds to the _response position_ identified by the _response path_ prefix ending at the same index: the value is {true} if that response position is non-null, and {false} otherwise.
>
> [In "Errors":] It must also contain an entry with the key {"pathNonNull"} with the _response path nullability_ for that path. This enables clients to implement advanced error handling behavior: for example, a client could issue a request using the {"NULL"} _error behavior_ and then reproduce any _error behavior_ locally, something that would otherwise require access to both the schema and the request document.

Example from the PR: `"path": ["hero", "heroFriends", 1, "name"], "pathNonNull": [false, true, false, false]` against `hero: Hero`, `friends: [Hero]!`, `name: String`. Note the unusual RFC 2119 weight: `pathNonNull` is a **must** on every error with a path, regardless of mode. It is contested: martinbonnin (2026-08-09): "About `pathNonNull` ... I am skeptical. Maybe we'll need it but I'd like to have this rooted into a real life use case before adding more to this proposal. Would be nice to ship `onError` as experimental and then add `pathNonNull` as an additive change if some teams need it." #1236 omits it. graphql-js #4364 implements it (tests show `pathNonNull: [false, true]` etc.).

## 4. What HALT means for partially-executed responses and mutations

Spec text: "aborted immediately", `data: null`, single error, "Any _response position_ that has not yet executed or has not yet yielded a value may be cancelled". So partially-executed work is discarded from the response; nothing in the proposal says side effects are rolled back or that in-flight resolvers are awaited.

**Mutations.** The proposal does not add any mutation-specific text. Serial root-field execution (`ExecuteMutation`) is untouched, so under `HALT` a mutation with root fields `a b c` where `b` errors returns `data: null` with only `b`'s error, and `a`'s side effect has already happened; `c` may or may not have started (spec: "may be cancelled"). Under `NULL` execution *continues* to `c`. This was raised in review (bbarry, 2026-05-01: "In the case of mutation resolution, does `"NULL"` mean execution continues?", with options A/B/C); benjie's answer (2026-05-02): the "continue resolving, `doThing3: true`" outcome "is the correct behavior according to the current semantics. It's an interesting question, because if the client changes the onError without the knowledge of the user, the resulting side-effects will differ." martinbonnin (2026-05-03) suggested recommending single-root-field mutations. No spec change resulted; the thread is unresolved and it is a design gap greem will have to take a position on (e.g. whether an error in a non-null mutation root field under `NULL` should stop later root fields).

**Subscriptions.** Explicitly: HALT aborts the current event's `ExecuteRootSelectionSet()` only; "the underlying stream is not terminated".

**Sibling cancellation and `errors` growth.** #1184 (RFC 2) changes graphql-js so that after propagation no further sibling errors are appended; the 2025-07-17 WG summary records Lee's concern "about the complexity of enforcing this rule in parallel processing scenarios", and the 2026-07-02 summary records it "has been resolved in version 17 by changing the behavior from a 'must' to a 'should' requirement". For a breadth-first executor this is the relevant rule: once a `PROPAGATE` null has been decided for an ancestor, later-arriving sibling errors should not be added to `errors`.

## 5. Interaction with non-null positions and incremental delivery

**Non-null.** Under `NULL`, "a `Non-Null` _response position_ will have the value {null} if and only if an error occurred at that position" — i.e. `!` becomes "null only on error" for that request. The blog draft: "From an error perspective, every position in the response (fields and lists alike) is an error boundary, as though it were nullable (for error-handling only)." The schema-side story (how to express "true" nullability while legacy `PROPAGATE` clients exist) is the separate Semantic Nullability RFC; the blog draft says the community has "mostly aligned on the use of the transitional `@semanticNonNull` directive" (`@semanticNonNull(levels: [...])`), and #1165's `@noPropagate` appendix is a stale strawman. None of that is in #1163/#1236.

**Incremental delivery.** Neither #1163 nor #1236 touches the incremental-delivery text (their diffs are confined to the non-incremental Section 6/7 paragraphs), and the incremental spec draft [#1110](https://github.com/graphql/graphql-spec/pull/1110) (RFC 2) predates `onError`. benjie on graphql-js #4364 (2026-08-13): "I've not attempted to reconcile it with incremental delivery yet, but I've tried to be careful in the wording to allow for it (e.g. for the `ABORT` mode it is essentially described as bubbling to the highest position it can, typically the operation root; but in incremental delivery it will be the individual incremental units)." The relevant #1110 text that `PROPAGATE` already composes with:

> If any _execution error_ were raised during the execution of the results in {"data"} and these errors propagated to a parent _response position_ of the _incremental object result_'s response position, the incremental object result is considered failed and should not be included in the incremental stream. The error that caused this failure will be included in an _incremental completion notice_.

So the expected (not yet specified) composition is: `NULL` — no propagation, every incremental result carries its own `errors`; `PROPAGATE` — as #1110 today; `HALT` — the unit of abort is the incremental result (deferred fragment / stream batch), not the whole response. The graphql-js v17 port (#4846, unmerged) implements `HALT` in `handleFieldError` simply as `throw error` regardless of type, which is exactly "bubble as far as the enclosing unit lets you".

## 6. Reference implementations

| Implementation | What ships | Where | Notes |
| --- | --- | --- | --- |
| **graphql-js 17.0.x (released 2026-06-15, latest 17.0.2)** | `@experimental_disableErrorPropagation` operation directive only | `src/type/directives.ts` (`GraphQLDisableErrorPropagationDirective`, locations QUERY/MUTATION/SUBSCRIPTION); `execute.ts` derives `errorPropagation = !operation.directives?.find(...)`; `Executor.handleFieldError` throws only when `this.validatedExecutionArgs.errorPropagation && isNonNullType(returnType)` | Merged via [#4348](https://github.com/graphql/graphql-js/pull/4348) (2025-02-24). Equivalent to `onError: "NULL"`. Not present in 16.x. No `HALT`, no `onError` argument. |
| graphql-js [#4364](https://github.com/graphql/graphql-js/pull/4364) (16.x.x) | full #1163 incl. `pathNonNull` | `onError?: 'NULL' \| 'PROPAGATE' \| 'ABORT'` on `ExecutionArgs`/`graphql()`; `handleFieldError`: PROPAGATE throws iff non-null, ABORT always throws, NULL never throws | **Open** since 2025-03-27, refreshed 2026-08-13 (still says `ABORT`). |
| graphql-js [#4842](https://github.com/graphql/graphql-js/pull/4842) (17.x.x, martinbonnin) | #1236 (`onError` only) | new `src/execution/ErrorBehavior.ts` (`'NULL' \| 'PROPAGATE' \| 'HALT'`); `errorBehavior = onError ?? (disablesErrorPropagation ? 'NULL' : 'PROPAGATE')`; invalid value → request error `"onError" must be one of "NULL", "PROPAGATE", or "HALT"`; tests: HALT "stops execution and reports only the halting error" | **Open** since 2026-07-22. Keeps the directive as an alias for `NULL`. |
| graphql-js [#4846](https://github.com/graphql/graphql-js/pull/4846) (17.x.x, benjie) | codex port of #4364 to v17 | | **Open** since 2026-08-13. |
| **graphql-ruby 2.6.11** | nothing | `guides/type_definitions/non_nulls.md` "Non-null error propagation" describes only classic bubbling; no `onError`/`disableErrorPropagation`/`semanticNonNull` in `lib/` or `guides/`; no matching issues | Not a reference implementation for this feature. |
| **graphql-java** | `@experimental_disableErrorPropagation` merged ([#3772](https://github.com/graphql-java/graphql-java/pull/3772), 2025-02-28, opt-in flag); `onError` request parameter **open** ([#4378](https://github.com/graphql-java/graphql-java/pull/4378), since 2026-05-11) | #4378: `enum OnError { NULL, PROPAGATE, HALT }` on `ExecutionInput`; `NonNullableFieldValidator`: PROPAGATE → throw `NonNullableFieldWasNullException`, HALT → `throw new AbortExecutionException(executionContext.getErrors())`; JVM-wide kill switch `Execution.setExperimentalOnErrorEnabled(false)`; directive still honored as `NULL`; test "with onError: HALT, execution stops and a request error is returned" expects `data == null`, one error with path | Note graphql-java's HALT yields the *accumulated* error list, not strictly one error. |
| **Hot Chocolate** | `onError` request property parsed and forwarded; modes **Propagate, Null only** | [#8612](https://github.com/ChilliCream/graphql-platform/pull/8612) (merged 2025-08-29) added `onError` parsing with PROPAGATE/NULL/HALT → `ErrorHandlingMode`; [#9470](https://github.com/ChilliCream/graphql-platform/pull/9470) "Add ErrorModes to HotChocolate" (merged 2026-03-28) **removed `Halt`**; `src/HotChocolate/Language/src/Language.Web/ErrorHandlingMode.cs` on main is `enum ErrorHandlingMode { Propagate = 0, Null = 1 }` | Cited by #1236 as a merged implementation, but HALT is not supported today. |
| **Apollo Kotlin** (client + `apollo-execution` server) | `enum class OnError { NULL, PROPAGATE, HALT }` (`@ApolloExperimental`) in `apollo-api`; `ApolloRequest.Builder.onError(...)` | [#6963](https://github.com/apollographql/apollo-kotlin/pull/6963) (merged 2026-06-08) "Previously only execution supported it. Moves `OnError` to `apollo-api`" | Client sends it as a request parameter. |

Client-side consumers named by the proposal for `NULL`: Relay (`@throwOnFieldError`, `@catch`), `graphql-toe`, `graphql-sock` (#1163 body; blog draft).

## 7. Open questions still live in the RFC (relevant to greem)

1. Whether `onError` lands alone (#1236 flavour) or with `pathNonNull` (#1163 flavour); whether a default other than `PROPAGATE` is ever allowed (#1163 says no, #1236 says implementation-defined).
2. Mutation semantics under `NULL` (continue vs stop after a failed non-null root field) — unresolved review thread.
3. `HALT` × incremental delivery — acknowledged unspecified; intended unit of abort is the incremental result.
4. `HALT` error count — spec says exactly one error; graphql-java returns all collected so far.
5. Discovery: `graphql.onError` service capability (#1208, RFC 1) vs out-of-band knowledge; spec says clients "should only include {onError} ... if it is known that the service supports this property" because some legacy servers reject unknown top-level request properties.
