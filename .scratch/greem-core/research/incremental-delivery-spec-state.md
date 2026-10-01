# Incremental delivery (`@defer` / `@stream`): spec state as of 2026-09-23

Ticket: [What is the current incremental delivery (@defer/@stream) spec shape?](https://github.com/XiNiHa/greem/issues/5)

Sources are primary only: graphql/graphql-spec (PRs and branch text), graphql/graphql-wg (RFC + meeting notes/agendas), graphql/defer-stream-wg (the sub-WG's decision log), graphql/graphql-over-http (spec + RFCs), graphql/graphql-js (v17 source at `main`). Snapshots used:

| Source | Ref |
| --- | --- |
| graphql-spec `incremental-integration` branch | `b57790d` (2026-06-15, "Fixes for incremenal delivery integration branch (#1232)") |
| graphql-spec PR #1110 head (`robrichard/defer-stream-v2`) | `045e193` (2026-08-18, "execution edits for defer and stream") |
| graphql-js `main` | `ee5ce41` (2026-09-09); `package.json` version 17.0.2 |
| graphql-over-http `main` | `3903e68` (2026-09-22) |
| graphql-wg `main` | `b7e922e` (2026-09-17) |

---

## 1. Which proposal is current

**Current = graphql-spec PR #1110 "Incremental Delivery spec draft" (Rob Richard), RFC stage 2 (label `📄 Draft (RFC 2)`), targeting `main`.** Its body: "Spec draft of `@defer` based on the response format in https://github.com/graphql/defer-stream-wg/discussions/69, with no duplication of fields. `@stream` will be added soon." Lee Byron moved it to RFC2 on 2026-03-05: "More work to do for sure, but we're waaaaay closer to the end than the start" ([#1110 comment](https://github.com/graphql/graphql-spec/pull/1110)).

The response format it encodes is the **"June 2023" format** from [defer-stream-wg discussion #69 "New response format (June 2023)"](https://github.com/graphql/defer-stream-wg/discussions/69): `pending` / `incremental` / `completed` arrays keyed by `id`, no field duplication, no overlapping branching. Everything earlier is dead:

- PR #742 "Spec edits for @defer/@stream" (2020, the `path`+`label`-per-payload "branching" format): CLOSED, labelled `🌱 Superseded (RFC X)` (closed 2025-06-12).
- Alternatives #1018 (benjie), #1023/#1026/#1034/#1052/#1054 (yaacovCR dedupe variants), #1074 (benjie's "Implementation of June 2023 incremental delivery format"): all CLOSED 2025-06-12 / 2025-07-02. #1020 "Batch deferred fields by defer path" is technically OPEN but untouched since 2023-03-20.
- The prose RFC lives at [graphql-wg/rfcs/DeferStream.md](https://github.com/graphql/graphql-wg/blob/main/rfcs/DeferStream.md), header "*Working Draft - September 2024*"; it already describes the `pending`/`incremental`/`completed` format but its examples are informal (numeric ids etc.). Treat it as background, not normative.

**Merge mechanics.** #1110 is being landed in slices onto a long-lived `incremental-integration` branch in graphql/graphql-spec (each slice body: "Extracted from the full PR (#1110) and targeting an integration branch to aid in review"):

| Slice | Section | Status |
| --- | --- | --- |
| [#1132](https://github.com/graphql/graphql-spec/pull/1132) | 3 Type System (directive definitions) | merged 2025-10-30 |
| [#1203](https://github.com/graphql/graphql-spec/pull/1203) | 7 Response + Appendix E Examples | merged 2026-04-09 |
| [#1223](https://github.com/graphql/graphql-spec/pull/1223) | 5 Validation | merged 2026-06-04 |
| [#1232](https://github.com/graphql/graphql-spec/pull/1232) | fixes (overlapping-stream validation, rename to "incremental completion notice") | merged 2026-06-15 |
| [#1234](https://github.com/graphql/graphql-spec/pull/1234) | 6 Execution: `CollectFields` | OPEN (opened 2026-07-15, +100/−26, benjie/robrichard review comments ongoing; on the 2026-09-17 secondary WG agenda: "Defer/Stream spec review (Remaining Time, Rob)") |
| (not yet sliced) | 6 Execution: plan generation, work queue, `@stream` | only in #1110 head |

`main` still contains **zero** occurrences of `@defer` (checked `Section 3 -- Type System.md` on `origin/main`). Prerequisite refactors already on `main`: #1039 `ExecuteCollectedFields` replaces `ExecuteSelectionSet` (2025-07-01), #1159 "execution result"/"request error result" terms (2025-06-26), #1135 "response stream" (2025-03-06), #1129 "Path" → own section (2025-02-06).

**Important gap:** the execution text at #1110 head (`045e193`) fully specifies `@defer` (defer usages, execution plans, execution groups, work-queue events) but **has no `@stream` execution algorithm** — `CompleteListValue` has no stream branch, `GetStreamUsage`/`initialCount` handling is absent, even though the work-queue event vocabulary (`STREAM_VALUES`, `STREAM_SUCCESS`, `STREAM_FAILURE`) and the Response section already cover streams. Where this note says "spec" for stream execution behaviour it is falling back to Section 7 (Response) text, defer-stream-wg decisions, and graphql-js.

WG cadence: the primary WG summaries mention the RFC in 2025-07, 2025-11, 2026-03-19 ("reviewing Rob's pull request for incremental delivery specifications", PR #1203), 2026-04-16 (validation section; defer/stream in subscriptions), 2026-05-07 ("Rob: Update the validation rules for defer/stream to use the more thorough validation approach"), 2026-06-04 (graphql-js v17 "introduces incremental delivery features under an experimental flag"). Nothing about it in the 2026-07..09 primary summaries; it is on the 2026-09-17 secondary (EU) agenda. The defer-stream-wg sub-group's own repo has agendas through `2025/04-Apr` and notes only for 2023; decisions are recorded as GitHub Discussions.

---

## 2. Directive definitions (integration branch, Section 3)

Normative text, `spec-integration/spec/Section 3 -- Type System.md`:

> GraphQL implementations may provide the `@defer` and/or `@stream` directives. If either or both of these directives are provided, they must conform to the requirements defined in this specification.

```graphql
directive @defer(
  if: Boolean! = true
  label: String
) on FRAGMENT_SPREAD | INLINE_FRAGMENT
```

> The `@defer` directive may be provided on a fragment spread or inline fragment to indicate that execution of the related selection set should be deferred. When a request includes the `@defer` directive, it may return an _incremental stream_ consisting of an _initial incremental stream result_ containing all non-deferred data, followed by one or more _incremental stream update result_ including deferred data.
>
> The `@include` and `@skip` directives take precedence over `@defer`.

Arguments:

> - `if: Boolean! = true` - When `true`, fragment _should_ be deferred (see Client Handling). When `false`, fragment must not be deferred. Defaults to `true`.
> - `label: String` - An optional string literal used by GraphQL clients to identify data in the _incremental stream_ and associate it with the corresponding defer directive. If provided, the GraphQL service must include this label in the corresponding _incremental pending notice_ within the _incremental stream_. The `label` argument must be unique across all `@defer` and `@stream` directives in the document. Variables are disallowed (via Defer And Stream Directive Labels Are Unique) because their values may not be known during validation.

```graphql
directive @stream(
  if: Boolean! = true
  label: String
  initialCount: Int! = 0
) on FIELD
```

> The `@stream` directive may be provided for a field whose type incorporates a `List` type modifier. The directive enables returning a partial list initially, followed by additional items in one or more _incremental stream update result_. If the field type incorporates multiple `List` type modifiers, only the outermost list is streamed.
>
> Note: The mechanism through which items are streamed is implementation-defined and may use technologies such as asynchronous iterators.
>
> The `@include` and `@skip` directives take precedence over `@stream`.

> - `if: Boolean! = true` - When `true`, field _should_ be streamed ... When `false`, the field must behave as if the `@stream` directive is not present—it must not be streamed and all of the list items must be included. Defaults to `true`.
> - `label: String` - (same as `@defer`)
> - `initialCount: Int! = 0` - The number of list items to include initially when completing the parent selection set. If omitted, defaults to `0`. An execution error will be raised if the value of this argument is less than `0`. When the size of the list is greater than or equal to the value of `initialCount`, the GraphQL service _must_ initially include at least as many list items as the value of `initialCount`.

"At least `initialCount`" (never fewer) is the outcome of [defer-stream-wg #104](https://github.com/graphql/defer-stream-wg/discussions/104): "discussed at the Feb 2025 primary WG and there was consensus to adopt Option 1 ... It is reasonable to allow a server to send more than the specified initialCount if, for example the server has them cached."

**Servers may ignore the directives** ("Client Handling of @defer/@stream"):

> It is highly recommended that GraphQL services honor the `@defer` and `@stream` directives on each execution. However, the specification allows advanced use cases where the service can determine that it is more performant to not defer and/or stream. Services can make this determination on case by case basis; e.g. in a single operation, one or more `@defer` and/or `@stream` may be acted upon while others are ignored. Therefore, GraphQL clients _must_ be able to process a _response_ that ignores individual `@defer` and/or `@stream` directives. This also applies to the `initialCount` argument on the `@stream` directive. Clients must be able to process a streamed field result that contains more initial list items than were specified in the `initialCount` argument.

Open nit (PR #1110 comment by duckki, 2026-09-20): whether `@defer(label: null)` should yield `"label": null` or omit it; graphql-js omits it (test "Treats null defer label the same as no label", `src/execution/incremental/__tests__/defer-test.ts`).

---

## 3. Validation rules (integration branch, Section 5)

Four new rules plus one change to Field Selection Merging:

1. **Defer And Stream Directives Are Used On Valid Root Field** — `ForbidDeferStream(selectionSet)` over the top-level selection set of every mutation and subscription operation: root fields must not carry `@stream`, root-level fragment spreads / inline fragments must not carry `@defer` (recursing through spreads). "The `@defer` and `@stream` directives are not allowed to be used on root fields of mutation or subscription operations." (Decision log: [defer-stream-wg #19](https://github.com/graphql/defer-stream-wg/discussions/19).)
2. **Defer And Stream Directives Are Used In Valid Operations** — for subscriptions, every `@defer`/`@stream` anywhere in the operation must have an `if` argument whose value is not the literal `false`... precisely: "{if} must be defined" and "{argumentValue} must not be the boolean value {false}" — i.e. the directive must be disable-able via a variable. "If these directives appear in a subscription operation they must be disabled using an `if` argument." At execution, `CollectFields` additionally says "If this execution is for a subscription operation, raise an _execution error_" when a defer is actually enabled. The 2026-04-16 WG summary calls this "an acceptable temporary workaround until a more comprehensive solution for subscription handling could be implemented".
3. **Defer And Stream Directive Labels Are Unique** — "{label} must not be a variable. {label} must not be present in {labelValues}." across the whole document (both directives share one namespace).
4. **Stream Directives Are Used On List Fields** — "Let {nullableFieldType} be the unwrapped nullable type of {adjacent}. {nullableFieldType} must be a List type."
5. **Field Selection Merging** gains `HasNoOverlappingStreams(fieldA, fieldB)`: "If neither {fieldA} nor {fieldB} has a directive named `stream`. Return {true}. Return {false}." — i.e. **two same-response-name field selections that could both apply to the same object are invalid if either carries `@stream`**, even with identical arguments. Decision: [defer-stream-wg #100 "Same stream across fragments"](https://github.com/graphql/defer-stream-wg/discussions/100): "The WG decided to propose expanding `OverlappingFieldsCanMerge` to prevent merging of any instances of `@stream` on the same field ... users blocked by this validation can use a field alias ... There was consensus at the Feb 2025 Primary WG on this approach". graphql-js implements it in `src/validation/rules/OverlappingFieldsCanBeMergedRule.ts` (`hasNoOverlappingStreams`, error text links to discussion #100).

graphql-js rule files: `DeferStreamDirectiveLabelRule.ts`, `DeferStreamDirectiveOnRootFieldRule.ts`, `DeferStreamDirectiveOnValidOperationsRule.ts`, `StreamDirectiveOnListFieldRule.ts` (`src/validation/rules/`).

---

## 4. Response format (integration branch, Section 7)

The response union gains a member:

> A _response_ is either an _execution result_, a _response stream_, an _incremental stream_, or a _request error result_.
>
> A GraphQL request returns an _incremental stream_ when the GraphQL service has deferred or streamed data as a result of the `@defer` or `@stream` directives. When the result of the GraphQL operation is an incremental stream, the first payload will be an _initial incremental stream result_, optionally followed by one or more _incremental stream update result_.

Note "optionally": the July 2025 WG ([defer-stream-wg #113](https://github.com/graphql/defer-stream-wg/discussions/113)) decided the spec describes a **stream whose first item differs in type** ("HTTP and other transports are expected to return the results as part of their byte stream"), while graphql-js may keep its `{ initialResult, subsequentResults }` map "as an implementation detail".

**Initial incremental stream result**

> must contain entries with keys {"data"}, {"pending"}, and {"hasNext"}, and may contain entries with keys {"errors"}, {"incremental"}, {"completed"}, and {"extensions"}.
>
> The value of {"hasNext"} must be {false} if the initial incremental stream result is the last response of the incremental stream. Otherwise, {"hasNext"} must be {true}.
>
> The value of {"pending"} must be a non-empty list of _incremental pending notice_.

Plus a non-normative note that a proxy/CDN may collapse a whole upstream incremental stream into a single initial result "containing the all of the intercepted incremental pending notices, incremental results, and incremental completion notices, and the {"hasNext"} entry set to false" — so `incremental`/`completed` may legitimately appear in the *initial* payload.

**Incremental stream update result**

> must contain an entry with the key {"hasNext"}, and may contain entries with the keys {"pending"}, {"incremental"}, {"completed"}, and {"extensions"}. Unlike the _initial incremental stream result_, an _incremental stream update result_ must not contain entries with keys {"data"} or {"errors"}.
>
> The value of {"hasNext"} must be {true} for all but the last response in the _incremental stream_. Otherwise, {"hasNext"} must be {true}.

(The second sentence is a typo in the merged text — intended `false`; the algorithm in Section 6 sets `hasNext` to `false` on `WORK_QUEUE_TERMINATION`.)

**Incremental pending notice** (`pending[]`)

> must contain entries with the keys {"id"} and {"path"}, and may contain an entry with key {"label"}.
>
> The value of {"id"} must be a string. ... The {"id"} value must be unique across the entire _incremental stream_ response.
>
> The value of {"path"} must be a _response position_. When the incremental pending notice is associated with a `@stream` directive, it indicates the list at this _response position_ is not known to be complete. ... When the incremental pending notice is associated with a `@defer` directive, it indicates that the response fields contained in the deferred fragment are not known to be complete.
>
> If an incremental pending notice is not returned for a `@defer` or `@stream` directive, clients must assume that the GraphQL service chose not to incrementally deliver this data, and the data can be found either in the {"data"} entry in the _initial incremental stream result_, or one of the prior _incremental stream update result_ in the _incremental stream_.

`path` for `@defer` is the position of the object the fragment is spread on (e.g. `["person"]`), for `@stream` the list field itself (`["person","films"]`). Ids are opaque strings; graphql-js uses a stringified counter (`IncrementalPublisher._ensureId`), the spec algorithm `EnsureID` does the same.

**Incremental result** (`incremental[]`) — either an *incremental list result* (`@stream`) or an *incremental object result* (`@defer`):

> Every _incremental result_ must contain an entry with the key {"id"} ... The associated incremental pending notice must appear either in the _initial incremental stream result_, in a prior _incremental stream update result_, or in the same _incremental stream update result_ as the _incremental result_ that references it.

Incremental list result:

> Every _incremental list result_ must contain an {"items"} entry. The {"items"} entry must contain a list of additional list items for the list field in the incremental list result's _response position_.

(No index is carried; items are appended in order — discussion #69 Example I: "No subpath or index, items must be returned in order. Multiple items can be returned in array".)

Incremental object result:

> may contain a {"subPath"} entry. If such an entry is present, the _response position_ of the incremental object result is the result of appending the value of this {"subPath"} to the value of the {"path"} entry of the _associated incremental pending notice_.
>
> An _incremental object result_ may be used to deliver data for response fields that were contained in more than one deferred fragment. In that case, the _associated incremental pending notice_ ... must be one of the _incremental pending notice_ that corresponding to a fragment that contained the delivered responsive fields. If any of these incremental pending notices have a {"path"} of varying length, one of the incremental pending notices with the longest {"path"} must be chosen to minimize the size of the {"subPath"}.
>
> Every _incremental object result_ must contain a {"data"} entry. The {"data"} entry must contain a map of additional response fields.

This is the "no duplication" property: a field selected by two overlapping `@defer`s is delivered exactly once, in an incremental object result attributed to *one* of them, and each fragment is only "done" when its `completed` notice arrives. Appendix E Example 2 states the consequence: "it is necessary for clients to process the entire incremental stream, as both the initial data and previous incremental results (with a potentially different value for {"id"}) may be required to complete a deferred fragment."

**Incremental completion notice** (`completed[]`)

> must contain an entry with the key {"id"}, and may contain an entry with the key {"errors"}.
>
> The value of {"errors"}, if present, informs clients that the delivery of the data from the _associated incremental pending notice_ has failed, due to an execution error propagating to a parent _response position_ of the _incremental result_'s response position.
>
> The corresponding data must have been completed in the same _initial incremental stream result_ or _incremental stream update result_ in which this incremental completion notice appears.

**Additional entries**: "any of the maps described in the 'Response' section (with the exception of {"extensions"}) must not contain any entries other than those described above. Clients must ignore any entries other than those described above."

Worked examples are normative-adjacent in `Appendix E -- Examples.md` (Example 1: defer + stream; Example 2: overlapping defers with `subPath`).

---

## 5. Delivery ordering guarantees

There is **no single normative sentence** "payloads must be ordered" in the current text. Ordering is enforced structurally:

- A `pending` notice must precede or accompany any `incremental`/`completed` that references its id (Section 7, quoted above).
- In the #1110 execution algorithm, deferred work only becomes *pending* when its parent finishes: `GROUP_SUCCESS` and `STREAM_VALUES` events carry `newGroups`/`newStreams`, which `GetPendingEntry` turns into `pending` entries in the same update result; and "Note: {executionGroupTask} can be safely initiated without blocking higher-priority data once any of {deferredFragments} are released as pending." (`CollectExecutionGroups`). Thus a nested deferred fragment or stream can never be delivered before the payload that contains its parent position.
- A deferred fragment is delivered atomically: `GROUP_VALUES` for a group is emitted only on `GROUP_SUCCESS`, i.e. when *all* execution groups contributing to that deferred fragment have completed (discussion #69: "Consistent delivery of fragments ... Even if 'MyFragment' is ready earlier, it is not sent until 'j' is also ready"). In graphql-js `WorkQueue.taskSuccess` a group is flushed only when `rootGroups.has(group) && groupNode.pending === 0`.
- Stream items are index-ordered by construction (a single item queue per stream; graphql-js `buildStreamItemQueue` increments `index` sequentially and pushes in order). The historical WG decision is [defer-stream-wg #17 "Enforcing delivery order of payloads"](https://github.com/graphql/defer-stream-wg/discussions/17): "responses like this must be ordered to ensure payload paths do not reference fields that have not been sent yet. Similarly, a streamed field result must not be sent before a result with a lower index."
- **Between independent deferred fragments / streams there is no order**: Appendix E Example 1: "Depending on the behavior of the backend and the time at which the deferred and streamed resources resolve, the stream may produce results in different orders."
- Batching is explicitly allowed: `BatchIncrementalResults` merges "one or more _incremental stream update result_ available" into one payload, "concatenating list entries as necessary, and setting {hasNext} to the value of {hasNext} on the final item". graphql-js caps stream batches at a default queue capacity of 100 items (`buildStreamItemQueue`, test "limits stream batches to the default capacity (100)").
- Whether deferred work *executes* before it is released as pending is implementation-defined: "Schedule initiation of execution of {executionGroupTask} following any implementation specific deferral." graphql-js exposes `enableEarlyExecution` ("Whether incremental execution may begin eligible work early", `src/execution/ExecutionArgs.ts`); default is to wait until released (`shouldDefer`, test "Does not execute deferred fragments early when not specified").

---

## 6. Errors and non-null propagation across the defer boundary

Two rules from Section 7 (quoted above) decide everything:

1. Errors that **stay inside** the deferred data go on the incremental result: "If any _execution error_ were raised during the execution of the results in {"data"} and no such error propagated to a parent _response position_ of the _incremental object result_'s response position, the incremental object result must contain an entry with key {"errors"} containing these execution errors."
2. Errors that **bubble to or past** the fragment's position kill the whole fragment: "If any _execution error_ were raised during the execution of the results in {"data"} and these errors propagated to a parent _response position_ of the _incremental object result_'s response position, the incremental object result is considered failed and should not be included in the incremental stream. The error that caused this failure will be included in an _incremental completion notice_."

So the already-delivered parent is **never rewritten**: a non-null violation inside `...F @defer` spread on `hero` that would have nulled `hero` produces *no* `incremental` entry and a `completed: [{ id, errors: [...] }]`; the client's `data.hero` stays as delivered. Discussion #69 Example H makes the rationale explicit for the sibling-defer case: "If a field in a subsequent defer nulls a previously sent field due to null bubbling, the entire fragment will not be delivered. Clients should treat this fragment similar to a fragment that is `@skip(if: true)`." and the FAQ: "If we delivered the other fields in the fragments it could put clients into a bad state where they understand that all the fields from defer 'B' have been delivered, but it has an object for 'bar' and no result for 'qux'... This should only happen when non-null fields are shared across sibling defers."

Execution side (#1110 head): `ExecuteExecutionGroup` returns `{data, errors, work}` for the deferred collected-fields map; the root algorithm notes "{ExecuteExecutionPlan()} does not directly raise execution errors from the incremental portion of the Execution Plan." The general Errors-and-Non-Null text keeps: "If this occurs, any sibling response positions which have not yet executed or have not yet yielded a value may be cancelled to avoid unnecessary work." The spec text does **not** yet spell out cancellation of deferred work whose position was nulled in the *initial* result (it follows from Section 7's "must not be included"); graphql-js does it explicitly: `IncrementalExecutor.getIncrementalWork` aborts tasks/streams where `collectedErrors.hasNulledPosition(task.path)` with reason "Cancelled secondary to null within original result".

graphql-js behaviour (tests in `src/execution/incremental/__tests__/defer-test.ts`):

- "Handles non-nullable errors thrown in deferred fragments": initial `{ data: { hero: { id: '1' } }, pending: [{ id: '0', path: ['hero'] }], hasNext: true }`, then `{ completed: [{ id: '0', errors: [ 'Cannot return null for non-nullable field Hero.nonNullName.' ... path: ['hero','nonNullName'] ] }], hasNext: false }` — no `incremental`.
- "Handles non-nullable errors thrown outside deferred fragments": the non-null failure is in the initial result, so `data.hero` is `null`, `errors` is on the initial payload, and the deferred fragment under `hero` is dropped entirely — the response is a plain `ExecutionResult` with **no `pending` at all** (no incremental stream).
- "Nulls cross defer boundaries, null first / value first": two defers share `a.b.c`; the one containing the non-null failure is completed with errors, the other still delivers `{ b: { c: {} } }` plus `{ subPath: ['b','c'], data: { d: 'd' } }`.
- "Cancels deferred fields when initial result exhibits null bubbling cancelling the defer", "Keeps deferred work outside nulled error paths", "Handles cancelling child deferred fragments if parent fragment fails" — cancellation is path-scoped, not global.

Interaction with `onError`/error-behaviour: not addressed in #1110 at all. graphql-js ships `@experimental_disableErrorPropagation` (`src/type/directives.ts`) and the separate spec PR #1163 "Error behaviors (including `onError: "NULL"`)" is still OPEN (see ticket 02).

---

## 7. `@stream` interaction with nullability and with the initial payload

Response-format rules (Section 7, incremental list result):

> If any _execution error_ were raised during the execution of the results in {"items"} and these errors propagate to the _response position_ of the _incremental list result_ (i.e. the streamed list), or a parent response position of the incremental list result's response position (i.e. a parent of the streamed list), the incremental list result is considered failed and should not be included in the _incremental stream_. The errors that caused this failure will be included in an _incremental completion notice_.
>
> If any _execution error_ were raised during the execution of the results in {"items"} and no such error propagated to the _response position_ of the _incremental list result_, or a parent response position of the incremental list result's response position, the incremental list result must contain an entry with key {"errors"} containing these execution errors.

Consequences, confirmed by graphql-js tests (`src/execution/incremental/__tests__/stream-test.ts`):

- **Nullable item type** (`[Friend]`, `[String]`): a failing item after `initialCount` is delivered as `null` inside `items` with the error on the same incremental entry: `{ incremental: [{ id: '0', items: [null], errors: [{ message: 'String cannot represent value: {}', path: ['scalarList', 1] }] }], completed: [{ id: '0' }] }` ("Handles errors thrown by completeValue after initialCount is reached").
- **Non-null item type** (`[Friend!]`): a null item after `initialCount` would null the *whole list*, which was already delivered in `data` — so the stream is terminated with `completed: [{ id: '0', errors: [{ message: 'Cannot return null for non-nullable field Query.nonNullFriendList.', path: ['nonNullFriendList', 1] }] }]` and no `incremental`; the client keeps the `initialCount` items it already has ("Handles null returned in non-null list items after initialCount is reached"; async-iterable variant returns the iterator). Discussion #69 Example J states the same rule: "If after some list fields are streamed: either the underlying datasource errors, or a null bubbles up to the list field — A 'completed' object is sent for the stream with the errors and no more streamed results will be sent."
- **Errors before `initialCount` is reached** are ordinary: they land in the initial `data`/`errors` and, for non-null items, null-propagate normally in the initial result (tests "Handles rejections in a field that returns a list of promises before initialCount is reached", "Handles error thrown in async iterable before initialCount is reached").
- A stream whose parent position is nulled in the initial result (or in a deferred payload) is dropped: "Filters payloads that are nulled" yields a plain `{ errors, data: { nestedObject: null } }` with no `pending`; "Filters stream payloads that are nulled in a deferred payload" / "Filters defer payloads that are nulled in a stream response" cover the cross cases; the source async iterator is `return()`ed ("Returns iterator and ignores errors when stream payloads are filtered").
- Only the outermost list is streamed (spec text; graphql-js: `typeof path.key === 'number' ? undefined : getStreamUsage(...)` in `Executor.completeAsyncIterableValue`/`completeListValue`).
- `initialCount` in graphql-js is exact, not "at least": the list loop hands the iterator to `handleStream` when `index === initialCount` (`Executor.ts`), for both arrays and async iterables. `initialCount < 0` → field error (`getStreamUsage` invariant; test "Negative values of initialCount throw field errors"). `label: null` treated as absent; `if: null` does **not** disable ("Does not disable stream with null if argument").
- `completed` for a stream may arrive in a later payload than the last item when the iterator's end is not known synchronously (Appendix E Example 1 third/fourth payloads; discussion #69 Example I).
- Streams inside deferred fragments and defers inside stream items are both allowed and nest through the same work queue ("Handles overlapping deferred and non-deferred streams", "Can @defer fields that are resolved after async iterable is complete", "Returns payloads in correct order when parent deferred fragment resolves slower than stream").

Two-phase execution model (#1110 head) worth knowing for greem's design: `CollectFields` now returns `(collectedFieldsMap, newDeferUsages)` where each *field detail* carries the `deferUsage` it was collected under; `BuildExecutionPlan(collectedFieldsMap, parentDeferUsages)` partitions response names by `GetFilteredDeferUsageSet` (fields selected outside any new defer, or whose defer set equals the parent's, execute now; the rest are grouped by *set* of defer usages, after removing usages whose ancestor is also present); `ExecuteExecutionPlan` runs the immediate map and, "allowing for parallelization", `CollectExecutionGroups` which creates one task per defer-usage-set; `GetNewDeferMap` materialises a *deferred fragment* (`{parent, path, label}`) per defer usage per concrete path (so a `@defer` under a list yields one pending entry per list item — graphql-js test "Returns payloads in correct order" shows `pending: [{ id: '1', path: ['hero','friends',0] }, ...]`). Field order within `data` is unaffected by defer (PR #1054's concern; `Note: {resultMap} is ordered by which fields appear first in the operation`).

---

## 8. Transports

**graphql-over-http normative spec (`spec/GraphQLOverHTTP.md`) says nothing about incremental delivery.** The only `multipart` mentions are CSRF cautions about `multipart/form-data` request bodies. Recently merged: `294 Partial Success` status for responses with both `data` and `errors` (PR #346, 2026-04-02) and "Introduce GraphQL-over-HTTP response" (#423, 2026-09-03) — neither covers streams.

**`rfcs/IncrementalDelivery.md`** (added by Rob Richard, PR #124, 2020; HTTP/2 note PR #252, 2023-05-19; last touch typo fix 2026-04-23) is the only over-HTTP text:

> An incrementally delivered response should contain the `Transfer-Encoding: chunked` response header when using HTTP/1.1. ... Because of improved data streaming mechanisms, HTTP/2 prohibits the use of the `Transfer-Encoding` header. ... Compliant servers must follow the HTTP/2 specification and not set the `Transfer-Encoding` header.
>
> The HTTP response for an incrementally delivered response should conform to the specification of multipart content defined by the W3 in rfc1341. The HTTP response must contain the `Content-Type` response header with a specified boundary, for example `Content-Type: multipart/mixed; boundary="-"`. A simple boundary of `-` can be used as there is no possibility of conflict with JSON data. However, any arbitrary boundary may be used.
>
> - Before each part of the multi-part response, a boundary (`CRLF`, `---`, `CRLF`) is sent.
> - Each part of the multipart response must contain a `Content-Type` header. ...
> - After all headers for each part, an additional `CRLF` is sent, followed by the payload for the part.
> - After the final payload, the terminating boundary of `CRLF` followed by `-----` followed by `CRLF` is sent.

Caveats: its example payload is the **superseded** format (`{"data":{...},"path":[],"hasNext":false}`); it does not define `Accept` negotiation (issue #167 "How is the response protocol determined in different scenarios", open since 2022, unanswered); parts are `application/json`, not `application/graphql-response+json`. The `deferSpec=20220824` `Accept` parameter seen in the wild is an Apollo convention, not in any graphql.org repo.

**SSE**: `rfcs/GraphQLOverSSE.md` (the `graphql-sse` protocol, PR #140-era) covers incremental delivery only in passing: distinct-connections mode requires `Content-Type: text/event-stream`, results are `next` events carrying an `ExecutionResult` followed by a `complete` event; "Streaming operations, such as `subscriptions` or directives like `@stream` and `@defer`, are terminated/completed by having the client simply close the SSE connection." Single-connection mode uses a reservation token and `DELETE ...?operationId=` to stop. Neither RFC has moved toward the normative spec; the WG's 2026-03-19 summary lists alternative encodings (Argo, CBOR) as a future over-HTTP topic, not incremental delivery.

---

## 9. graphql-js: what the reference implementation does

- **Version**: v17.0.0 released 2026-06-15 (after `17.0.0-rc.0` 2026-06-02); latest v17.0.2 (2026-07-03). The 2026-06-04 WG summary: v17 "introduces incremental delivery features under an experimental flag".
- **Opt-in**: `GraphQLDeferDirective` / `GraphQLStreamDirective` are exported but "not included in `specifiedDirectives`"; a schema must add them. `execute()` "does not support incremental delivery" and `executeImpl` **throws** if the schema even defines them: "The provided schema unexpectedly contains experimental directives (@defer or @stream). These directives may only be utilized if experimental execution features are explicitly enabled." (`src/execution/execute.ts`). Internally an `ExecutorThrowingOnIncremental` aborts with "Executing this GraphQL operation would unexpectedly produce multiple payloads".
- **Entry points** (`src/execution/index.ts`): `experimentalExecuteIncrementally(args)` → `ExecutionResult | ExperimentalIncrementalExecutionResults { initialResult, subsequentResults: AsyncGenerator }` (the map form; see §4). `legacyExecuteIncrementally(args)` → the **old branching format** ("each subsequent incremental payload identifies its location with `path` and optional `label` fields. The current format instead tracks pending work by `id` and reports completion through `completed` entries", `src/execution/legacyIncremental/legacyExecuteIncrementally.ts`) for clients (Relay/Apollo era) still on PR #742's shape. Its doc comment still cites PR #742 as the algorithm source even though the payloads are the #1110 format.
- **Architecture** (`src/execution/incremental/`): `buildExecutionPlan.ts` (mirror of `BuildExecutionPlan`), `IncrementalExecutor.ts` (subclass of `Executor` that overrides `executeCollectedRootFields`/`executeCollectedSubfields`/`handleStream`; types `DeliveryGroup {path,label,parent}` = spec "deferred fragment", `ItemStream {path,label,initialCount,queue}`, `ExecutionGroupValue {deliveryGroups,path,data,errors}`), `WorkQueue.ts` (generic graph of groups/tasks/streams emitting exactly the spec's `GROUP_VALUES | GROUP_SUCCESS | GROUP_FAILURE | STREAM_VALUES | STREAM_SUCCESS | STREAM_FAILURE | WORK_QUEUE_TERMINATION` events), `IncrementalPublisher.ts` (maps events to `pending`/`incremental`/`completed`, assigns string counter ids, picks `bestId`/`subPath` by longest pending path — `_getBestIdAndSubPath`). The spec's Section 6 algorithms are essentially a transcription of this code.
- **Implementation-defined choices greem must make too**: `enableEarlyExecution` (default off: deferred groups and stream iteration start only once released as pending); stream item batching with default capacity 100 and `push` back-pressure; `abortSignal` cancels all outstanding groups/streams and `return()`s source async iterables; a per-stream item is completed by a sub-`Executor` so nested `@defer`/`@stream` inside an item produce their own work; `initialCount` honoured exactly; pending ids assigned in first-seen order.
- **Open graphql-js work tracking the spec**: PR #4841 "Skip deferred fragments in collectFields only when same directive is used" (OPEN) — fixes the `visitedFragments` check so `...F @defer(label:"A") ...F @defer(label:"B")` yields two pending entries and a non-deferred `...F` after a deferred one is not dropped; this is the same "visited fragment state" (map of fragment name → set of defer directive nodes, `null` = visited undeferred) that spec PR #1234 introduces. Both were on the 2026-09-17 secondary WG agenda.

---

## 10. What is still open (as of 2026-09-23)

1. `@stream` **execution algorithm** text is absent from #1110; the only normative stream text is Section 3 (arguments), Section 5 (validation) and Section 7 (response). Behaviour is defined by graphql-js + discussion #69/#104.
2. Section 6 execution slices (#1234 CollectFields, then plan generation / work queue) are not on the integration branch; #1234 is under review.
3. Subscriptions: incremental delivery is forbidden via the "must be disable-able with `if`" validation rule; a real design is deferred ("temporary workaround", 2026-04-16 WG).
4. `label: null` serialisation (#1110 comment 2026-09-20). `hasNext` typo in the update-result paragraph.
5. Transport: no normative HTTP text; `multipart/mixed` RFC is stale (legacy example, no `Accept` negotiation); SSE mentions defer/stream only for termination.
6. Interaction with error-behaviour/`onError` (#1163) is unspecified; the integration branch predates any of that work.
7. graphql-js still calls the API "experimental" and gates it behind separate entry points; nothing in the v17 release notes promises stability of the payload types.
