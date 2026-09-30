# Lookbehind planning in practice: Cardinal PoC, graphql-ruby `Execution::Next`, Grafast

Ticket: [../issues/04-lookbehind-planning-in-practice.md](../issues/04-lookbehind-planning-in-practice.md)
Date: 2026-09-23

## Sources (pinned)

| Tag | Source | Pinned at |
|---|---|---|
| [C] | `gmac/graphql-breadth-exec` (Shopify Cardinal PoC), README + `lib/` + `test/` | commit `a1dbc567dd3fc557a4d413fa4f2c48382c7864ec` (2026-07-19) |
| [R] | `rmosolgo/graphql-ruby` `lib/graphql/execution/*.rb`, `lib/graphql/dataloader.rb`, `guides/execution/next.md` (served as graphql-ruby.org/execution/next.html), `guides/execution/migration.md` | commit `30ff4107ba98d1c8aa4e85c54991f0ec090ec4c1`, `GraphQL::VERSION = "2.6.11"` |
| [G] | `graphile/crystal` `grafast/grafast/src/**` and `grafast/website/grafast/*.mdx` (served as grafast.org/grafast/...) | commit `566caa3802f2ea3ab8e85a02842e331d17711fb8`, grafast `1.1.3` |
| [S] | Shopify Engineering, "Faster breadth-first GraphQL execution" (the map's source article) | fetched 2026-09-23 |

File references below are `repo:path:lines` at those commits. Excerpts are verbatim, trimmed with `...` where noted.

The term "lookbehind" is the article's: "After tree building, Cardinal runs a bottom-up planning pass—heavily inspired by Grafast. ... We offer this _lookbehind_ pass as an alternative to lookahead, because lookahead cannot make informed choices about unresolved abstracts below it." [S]. The PoC README says the same in its own words: "**An execution tree can only be traversed from the bottom-up**. This is extremely intentional, because traversing top-down can never see through unresolved abstractions." [C] `README.md:109`.

Headline: only Cardinal has a per-request bottom-up planning pass. graphql-ruby's `Execution::Next` (Cardinal's open-source descendant) has **no planning hook at all** — it kept the breadth-first executor and dropped `plan`/`preload`. Grafast plans once per operation, top-down/breadth-first, and gets its "children inform ancestors" effect from the `optimize` lifecycle (which runs dependents-first) plus `deduplicatedWith`/`finalize` attribute merging.

---

## 1. Cardinal PoC (`graphql-breadth-exec`) [C]

### 1.1 The three phases, and when planning runs

README `## Query planning` (`README.md:491-499`):

> 1. The execution tree is built from top-down, omitting abstract positions.
> 2. A planning pass runs from bottom-up on the constructed tree. Fields may register actions on their ancestors.
> 3. The final execution pass runs from top-down, performing planned actions when encountered.
>
> These three phases repeat each time an abstract position is resolved to build, plan, and execute its resulting subtree.

Implementation, `lib/graphql/breadth/executor/execution_planner.rb:102-116`:

```ruby
def plan_scopes(scopes)
  scopes.delete_if { _1.objects.empty? }
  scopes.freeze
  return scopes if scopes.empty?

  scopes.each do |exec_scope|
    # invoke planning hooks from bottom-up to bubble configuration...
    build_execution_tree(exec_scope).reverse_each do |exec_field|
      exec_field.resolver.plan(exec_field, @context)
    end
  end

  scopes
end
```

`build_execution_tree` returns `ordered_fields`, a pre-order (parent-before-children) DFS list built while constructing the tree; `reverse_each` therefore visits every descendant before its ancestor. So "bottom-up" is literally *reverse construction order*, not a separate traversal. The pass is **synchronous and happens once, per root scope, before the first field executes**: `Executor#execute` does `@planner.plan_scopes(root_scopes).each { |exec_scope| @exec_queue << exec_scope; run! }` (`executor.rb:290-295`). Mutations get one root scope per top-level field, each planned+run serially (`execution_planner.rb:75-86`, `executor.rb:292-295`).

Timing relative to objects: at planning time the tree has **no objects below the root** — child scopes are created with `objects: [], results: []` (`execution_planner.rb:311-319`) and only filled when the parent field's result is built (`executor.rb:726-731`). Planning therefore sees types, selections, arguments and tree shape, never data. (Scope objects arrive just before that scope executes; see `on_preload` below for the just-in-time hook.)

### 1.2 Primitives a field can register

All planning primitives live on `ExecutionField` and `ExecutionScope`, via two mixins: `LazyElement` (preloads) and `HasAttributes` (notes).

**`plan` hook** — `lib/graphql/breadth/field_resolvers.rb:6-15`:

```ruby
class FieldResolver
  #: (Executor::ExecutionField[untyped], GraphQL::Query::Context) -> void
  def plan(_exec_field, _ctx)
    nil
  end

  def resolve(exec_field, ctx)
    raise NotImplementedError, "FieldResolver#resolve must be implemented."
  end
```

Return value is discarded (README `:503`). Note the resolver is a per-field *singleton object* (`f.breadth_resolver = MyFieldResolver.new`, README `:136-140`); all per-request state must go on the `exec_field`/`scope`, not on the resolver.

**`preload`** — `lib/graphql/breadth/executor/lazy_element.rb:45-63`:

```ruby
def preload(loader_class, args: nil, keys: nil)
  unless allows_preload?
    Kernel.raise LazySequencingError.new(lazy_element: self, method_name: "preload")
  end

  if keys
    loader = executor.lazy_loader_for(loader_class, args)
    loader.load(element: self, keys: keys).with_registry(preload_promises)
  else
    @lazy_preloads ||= {}
    deferred = (@lazy_preloads[[loader_class, args]] ||= ExecutionPromise::Deferred.new(registry: preload_promises))
    deferred.promise
  end
end
```

Two forms: with explicit `keys` (bound to a loader immediately) or without (a `Deferred` promise keyed by `[loader_class, args]`, bound later with the element's own `objects` as keys). Both return an `ExecutionPromise` that the planner can `.then` on. `LazyLoader` instances are cached per executor by `[loader_class, args]` (`executor.rb:185-189`), so any two fields anywhere in the tree that preload the same `(class, args)` share one batch.

**`on_preload`** — `lazy_element.rb:35-43` — registers a sync block run at execution time, immediately before the element's preloads are bound, when `objects` are finally known:

```ruby
def on_preload(&block)
  unless allows_preload?
    Kernel.raise LazySequencingError.new(lazy_element: self, method_name: "on_preload")
  end

  @sync_preloads ||= []
  @sync_preloads << block
end
```

**`attributes` / `attribute(key)` / `attribute?(key)`** — `lib/graphql/breadth/executor/has_attributes.rb` — a lazily-allocated freeform `Hash` on every field, scope and directive; README calls it "a hash intended for local caching and freeform planning notes" (`README.md:94`). Ancestor annotation example from README `:604-611`:

```ruby
def plan(exec_field, context)
  ancestor_scope = exec_field.scope&.parent
  if ancestor_scope && ancestor_scope.parent_type == Sprocket
    ancestor_scope.attributes[:include_widgets_sql] = true
  end
end
```

**`mutable_arguments`** — `execution_field.rb:206-208` — a deep copy of the coerced arguments that planning/directives may edit (`README.md:88`).

**`planning_root`** / **`allows_preload?`** — the ancestor-navigation guards (see 1.4).

Scope vs field preload differ only in *what they block*: "Preloading a _scope_ will block entering the scope until its preloads are complete; preloading on a _field_ will only block the field itself while allowing sibling fields in the scope to be traversed, thus allowing the discovery of other batching targets among sibling subtrees." (`README.md:547`).

### 1.3 Lifecycle guard: a three-state machine on every element

`lazy_element.rb:9-17,31-33,111-116` and `execution_field.rb:141-144,245-249`:

- `:preloading` (initial) — `preload`/`on_preload` allowed; `lazy` forbidden.
- `:executing` (fields only; set in `execute_field` right before `resolver.resolve`) — `lazy` allowed; `preload` forbidden.
- `:locked` (after result built / scope entered) — nothing allowed.

Violations raise `LazySequencingError` (`errors.rb:260`), i.e. **the plan/resolve separation is enforced at runtime by exceptions**, not by types. README `:543`: "Calling `preload` within a field resolver after execution starts will raise a `LazySequencingError`."

### 1.4 Abstract types: omitted, then planned lazily as a sub-tree

Tree building stops at abstract return types (`execution_planner.rb:303-322`):

```ruby
def add_execution_field_branch(exec_field, ordered_fields)
  exec_field.scope.fields[exec_field.key] = exec_field
  ordered_fields << exec_field

  return_type = exec_field.type.unwrap
  return if return_type.kind.leaf? || return_type.kind.abstract?

  next_scope = ExecutionScope.new(... parent_type: return_type, parent_field: exec_field,
    selections: exec_field.selections, objects: [], results: [])
  @planned_scopes_by_field[exec_field] = next_scope
  build_execution_tree(next_scope, ordered_fields)
end
```

When the abstract field's result is built, `build_abstract_scopes` (`executor.rb:901-961`) resolves a concrete type *per object* (via a `"__type__"` resolver in the resolver map or `schema.resolve_type`), buckets objects and result hashes by concrete type, wraps the buckets in an `AbstractExecutionScope` bookkeeping object, creates one `ExecutionScope` per concrete type with the *same* selections, and then re-enters phases 1–2 for the new sub-trees before queueing them:

```ruby
next_objects_by_type.each do |impl_type, impl_type_objects|
  scopes << ExecutionScope.new(executor: self, abstraction: abstract_scope, parent_field: exec_field,
    parent_type: impl_type, selections: exec_field.selections, ..., objects: impl_type_objects,
    results: next_results_by_type[impl_type])
end

@exec_queue.concat(@planner.plan_scopes(scopes))
```

Consequences for planning:

- Fields under an abstract position are planned **after** everything above them has already executed. Their `plan` hooks can still annotate ancestors' `attributes`, but ancestors are `:locked` so `preload` on them raises. README `:587`: "abstract selection branches are planned _lazily after resolution_, at which time the document above their subtree has been sealed and no longer accepts preloads."
- Hence `planning_root` (`execution_scope.rb:96-99`): `@planning_root ||= (abstraction || @parent.nil?) ? self : @parent.planning_root` — "always locate the highest unplanned scope and operate there" (README `:587-593`), and `allows_preload?` "always returns false for taxonomy above the current `planning_root`" (`README.md:597`).
- Because the abstract sub-tree scopes are typed concretely, planning inside them is exactly like planning anywhere else; there is no "polymorphic planning" concept. The cost is that fragments on `Animal` are re-planned once per concrete type present in the data (not once per possible type).
- Fragment inclusion at tree-build time is by `possible_types` check (`execution_planner.rb:213-215`), so a scope for `Cat` only contains fields applicable to `Cat`.

### 1.5 How planned preloads are delivered to resolvers

There is **no automatic injection**. A preload is a promise; the planner attaches a `.then` that stashes the result somewhere the resolver will look — canonically `exec_field.attributes` or `exec_field.scope.attributes`. From `test/graphql/breadth/executor/loaders_test.rb:581-597`:

```ruby
class FieldPreloadResolver < GraphQL::Breadth::FieldResolver
  def plan(exec_field, _ctx)
    exec_field.on_preload do
      exec_field.preload(
        FancyLoader,
        args: { group: "field" },
        keys: exec_field.objects.map { _1["first"] },
      ).then do |values|
        exec_field.attributes[:preloaded_values] = values
      end
    end
  end

  def resolve(exec_field, _ctx)
    exec_field.attributes[:preloaded_values]
  end
end
```

Sequencing that makes this work (`executor.rb:618-633`, `lazy_element.rb:65-82`, `executor.rb:341-355`):

1. `execute_field` calls `exec_field.preload!`, which (a) drains `@sync_preloads` (`on_preload` blocks, which may call `preload` because the state is still `:preloading`), then (b) binds each keyless `Deferred` preload to a loader using `keys: objects`.
2. If any preload promise exists, the field is pushed to `@lazy_queue`, a placeholder (`UNDEFINED`) is written into every result hash to preserve key order, and the resolver is *not* called yet.
3. `run!` drains `@exec_queue` first (whole generation of scopes), then swaps out the entire `@lazy_queue` and calls `execute_lazy`, which performs every loader that has `promised` entries **once** (`executor.rb:456-488`), settles the promises (running the `.then` blocks, hence populating `attributes`), and resumes each element: fields re-enter `execute_field` (now with preloads resolved, so `resolver.resolve` runs), scopes re-enter `execute_scope`.

A loader's `load` returns one promise per *element* bound to the element's whole key set — README: "a breadth LazyLoader binds entire key sets to a single promise, rather than building 1:1 promises" (`README.md:261`). Results are collected positionally (`lazy_loader.rb:214-230`); `nil` keys are skipped by default; `eager_values` short-circuit the batch per field instance.

### 1.6 Lists and arguments

Lists: the tree has one `ExecutionScope` per composite field regardless of list nesting; list wrappers stay on `exec_field.type`. At result-build time `build_and_flatmap_composite_result` (`executor.rb:760-778`) recursively walks list nesting and appends every non-nil object into flat `next_objects`/`next_results` arrays, which become the child scope's index-aligned `objects`/`results`. So a scope's `objects` under `products { nodes { ... } }` are all product nodes across all parents, and preloading on that scope batches across them. The planner never sees list cardinality (objects are empty during planning); only `on_preload` does.

Arguments: coerced eagerly when the `ExecutionField` is constructed, from the *first* merged field node (`execution_field.rb:69`), so `exec_field.arguments` is available inside `plan`. Arguments are per selection, shared by all objects in the scope; argument `prepare` hooks are intentionally unsupported (`README.md:32,87`). Argument validation errors are surfaced only at execution (`validate!` in `execute_field`, `executor.rb:642`). Loader `args:` (constructor kwargs, e.g. `{ association: :sprockets }`) are part of the loader cache key, so different arguments produce different batches.

### 1.7 Concurrency model

Everything is a single synchronous loop (`run!`). I/O parallelism is opt-in through the `async` gem: a `LazyLoader` class calls `async resource:, limit:, timeout:, throttle:`; `execute_lazy` splits batches into sync and async and runs the async ones under an `Async::Barrier` with per-resource `Async::Semaphore`s (`lazy_async.rb:217-245`). Requeued lazy work (chains) is scheduled while other async loaders are still in flight (`lazy_async.rb:278-335`). Subscription `subscribe` must return synchronously (`README.md:851`).

---

## 2. graphql-ruby `GraphQL::Execution::Next` [R]

### 2.1 What it is

`guides/execution/next.md:17-19`: "Breadth-first GraphQL execution (or, "execution batching") is an algorithmic paradigm developed by Shopify ... The original proof-of-concept ... can be found in graphql-breadth-exec. That prototype matured into Shopify's proprietary _GraphQL Cardinal_ execution engine". Enabled with `use GraphQL::Execution::Next` + `Schema.execute_next` (`next.md:35-38`).

### 2.2 No planning pass

`grep -rn "plan\|preload"` over `lib/graphql/execution/*.rb` finds nothing except `:lookahead` (`field_resolve_step.rb:217`). The public docs (graphql-ruby.org/execution/next.html, fetched 2026-09-23) mention neither "plan" nor "preload". The only forward-looking primitive is the pre-existing top-down `Execution::Lookahead` via `extras: [:lookahead]` (`lookahead.rb:4-29`) — the exact thing the article's lookbehind was meant to replace. There is no `attributes` bag on the execution steps either.

What replaces it is a *scheduler*, not a planner: every unit of work is a step object appended to the Dataloader job queue.

### 2.3 Execution shape (for comparison with Cardinal's scopes/fields)

- `SelectionsStep` ≈ Cardinal scope: `parent_type`, `objects`, `results`, `selections`; `call` groups selections by response key into `FieldResolveStep`s and `add_step`s each (`selections_step.rb:30-92`).
- `FieldResolveStep` ≈ Cardinal field, but it is a resumable state machine driven by which ivars are set (`field_resolve_step.rb:82-106`):

```ruby
def call
  return nil if @selections_step.killed
  set_current_field if @field_definition

  if @enqueued_authorization
    enqueue_next_steps
  elsif @finish_extension_idx
    finish_extensions
  elsif @field_results
    build_results
  elsif @arguments
    execute_field
  else
    build_arguments
  end
```

- Resolution modes (`resolve_batch` in `field_resolve_step.rb:686-797`): `:resolve_batch` (`objects, context, **args` → same-size array), `:resolve_static` (`Array.new(objects.size, result)`), `:resolve_each`, `:hash_key`, `:direct_send`, `:dig`, `:dataload` (`context.dataload_all(Source, objects)`), `:resolver_class` (one step per object), `:resolve_legacy_instance_method`. Chosen at field-definition time (`schema/field.rb:272-297`).
- Queue: `Runner#add_step` → `@dataloader.append_job(step)` (`runner.rb:51-53`); `Runner#execute` loops `isolated_steps` (one group for queries, one per mutation field) calling `@dataloader.run` (`runner.rb:119-126`). Ordering is FIFO job order, so it is breadth-first by construction, but there is no explicit generation barrier; the dataloader interleaves jobs and source fibers.
- Lazies (GraphQL-Batch promises, lazy `authorized?`, lazy `resolve_type`) are parked by depth: `@runner.dataloader.lazy_at_depth(path.size, self)` (`field_resolve_step.rb:368`), and the dataloader resumes the **shallowest** depth first once jobs are exhausted (`dataloader.rb:292-304`: `smallest_depth = lazies_at_depth.each_key.min`).

### 2.4 Abstract types

Resolved per object at runtime inside `enqueue_next_steps` (`field_resolve_step.rb:546-584`):

```ruby
if @static_type.kind.abstract?
  next_objects_by_type = Hash.new { |h, obj_t| h[obj_t] = [] }.compare_by_identity
  next_results_by_type = Hash.new { |h, obj_t| h[obj_t] = [] }.compare_by_identity

  @all_next_objects.each_with_index do |next_object, i|
    result = @all_next_results[i]
    if (object_type = @runner.runtime_type_at[result])
      # OK
    else
      ...
      object_type = ResolveTypeStep.resolve_type(@static_type, next_object, query)
      ...
      @runner.runtime_type_at[result] = object_type
    end
    next_objects_by_type[object_type] << next_object
    next_results_by_type[object_type] << result
  end

  next_objects_by_type.each do |obj_type, next_objects|
    @runner.add_step(SelectionsStep.new(path: path, field_resolve_step: self, parent_type: obj_type,
      selections: @next_selections, objects: next_objects, results: next_results_by_type[obj_type], ...))
  end
```

Same bucketing-by-concrete-type as Cardinal; the concrete type is remembered in an identity-keyed side table `runtime_type_at[result_hash]` (`runner.rb:8`) that the post-hoc `Finalize` pass uses to re-walk fragments (`finalize.rb:103-105`). Lazy `resolve_type` is `sync`ed inline per object with a `# TODO batch this` (`field_resolve_step.rb:560-563`).

### 2.5 Lists, arguments, delivery

- Lists: `build_graphql_result` (`field_resolve_step.rb:623-684`) recursively flattens list nesting into `@all_next_objects`/`@all_next_results`, as Cardinal does. When per-object authorization or type resolution is needed, each object becomes a `PrepareObjectStep` (`prepare_object_step.rb`) and the field step waits on `@pending_steps` before enqueueing the child `SelectionsStep`.
- Arguments: built per field step in `build_arguments` (`field_resolve_step.rb:169-186`), before any object is known ("arguments are prepared before objects are ready", `migration.md:280`). `loads:` arguments become asynchronous `LoadArgumentStep`s that re-`add_step` the field once all have landed (`load_argument_step.rb:94-98`). Arguments are passed to batch methods as kwargs: `public_send(key, objects, context, **args_hash)`.
- Delivery of preloaded data: none in the framework. Batching is entirely the resolver's job via `context.dataload_all(Source, objects)` / `dataload_all_records` / `dataload_all_associations` inside a `resolve_batch:` method (`next.md:146-157`, `field_resolve_step.rb:746-773`), which parks the fiber until the Dataloader runs pending sources.

### 2.6 Dynamic-language / synchronous assumptions

- Fiber-based Dataloader (`dataloader.rb:209-256`, `Fiber[:__graphql_current_multiplex]`, `Fiber[:__graphql_current_field]`) for both batching and lazies — the executor blocks inside a step and is resumed by the scheduler.
- Identity-keyed hashes on *result hash objects* (`compare_by_identity` for `runtime_type_at`, `static_type_at`, `finalizers`) — the result tree doubles as the key space for side tables.
- Steps mutate their own ivars to record progress and re-enqueue themselves (`@runner.add_step(self)`).
- `resolve_type`/`authorized?` may return lazies and are synced one object at a time.

---

## 3. Grafast [G]

### 3.1 When planning runs

`operation-plan.mdx:18-28`: "When Grafast sees an operation for the first time, it builds an _operation plan_ ... it walks the selection sets calling the developer-provided field plan resolver (`plan` method) for each field to determine the _steps_ ... Finally the _execution plan_ is optimized and finalized and the _output plan_ is finalized, then they are ready for execution." Plans are cached and reused across requests: "planning does not have access to the raw input values, instead representing them as steps to be populated at execution time for each request" (`plan-resolvers/index.mdx:19-24`). So planning is **per operation, before any execution, with no values** — different from Cardinal (per request, before execution, no values) and from both Ruby engines in being reusable.

The traversal is explicitly breadth-first by *planning depth*. `operation-plan.mdx:30-48` gives the algorithm ("While `nextSelections` is not empty: ... Call the field's plan resolver. Call any uncalled argument applyPlan resolvers. **Deduplicate** new steps. If the field has a selection set: Add all selections ... to `nextSelections`" then "Tree shake. Optimize. Tree shake. Finalize."). Source: `engine/OperationPlan.ts:1636-1750` `planPending()`:

```ts
private planPending() {
  for (let depth = 0; depth <= this.maxPlanningDepth; depth++) {
    // Process the next batch
    const l = this.planningQueue.length;
    if (l === 0) break;
    ...
    const batch = this.planningQueue.slice(0, l);
    const todo: Todo = [...this.planningQueueByPlanningPath.entries()];
    this.planningQueue.length = 0;
    this.planningQueueByPlanningPath.clear();

    // First, do a planning-path-aware deduplicate
    this.deduplicateSteps();
    ...
    // Then, apply the per-method batch tweaks (such as resolving polymorphism)
    this.mutateTodos(todo);
    ...
```

Phases are tracked as a string state (`this.phase = "plan" | "validate" | "optimize" | "finalize" | "ready"`, `OperationPlan.ts:438-552`); some step methods branch on `this.operationPlan.phase === "plan"` (`steps/loadOne.ts:251`).

### 3.2 Primitives a field registers: steps and dependencies

A field's plan resolver returns one step (`interfaces.ts:310-320`):

```ts
export type FieldPlanResolver<TSourceStep, TArgs, TResultStep> = (
  $source: TSourceStep,
  fieldArgs: FieldArgs<TArgs>,
  info: FieldInfo,
) => TResultStep | null;
```

Steps register their inputs with `addDependency`/`addUnaryDependency`, which return an index into the `values` tuple received at execution (`step-classes.mdx:735-757`). Dependencies may only point to the same or a shallower layer plan (`step.ts:682-690`):

```ts
protected _addDependency(options: AddDependencyOptions): number {
  assertStep(options.step, () => `${this}._addDependency`);
  if (options.step.layerPlan.id > this.layerPlan.id) {
    throw new Error(
      `Cannot add dependency ${options.step} to ${this} since the former is in a deeper layerPlan (...; creates a catch-22)`,
    );
  }
  return this.operationPlan.stepTracker.addStepDependency(this, options);
}
```

Where Cardinal has one "preload" primitive, Grafast has the whole step vocabulary; the closest analog to a preload is `loadOne`/`loadMany` (`steps/loadOne.ts`, `steps/loadMany.ts`): a step whose `execute` batches all specs in the bucket, dedupes them, tick-batches across *other* `LoadOneStep`s sharing the same callback (`_loadCommon.ts:216-238`, `nextTick(() => executeBatches(...))`), and caches by spec in `extra.meta` keyed by `metaKey` (`loadOne.ts:230`).

### 3.3 Where the "lookbehind" effect lives: children informing ancestors

Grafast has no bottom-up planning *traversal*; ancestors are informed through three lifecycle mechanisms, all of which run after the breadth-first walk:

1. **Plan-time attribute requests.** Calling `$loadOne.get("name")` during a child's plan resolver mutates the parent step (`loadOne.ts:247-262`): `this.attributes.add(attr); return access(this, attr);`. The parent's `execute` later receives `info.attributes` so the callback can select only what was asked for. This is the same intent as Cardinal's `ancestor_scope.attributes[:include_widgets_sql] = true`, but expressed as a method on the parent step rather than a freeform bag.
2. **`deduplicate` / `deduplicatedWith`.** After each planning layer, peers (same class, same deps) are merged; the discarded step gets to pass its requirements to the survivor (`step-classes.mdx:318-375`). `loadOne.ts:179-193`:

```ts
public deduplicate(peers: readonly LoadOneStep<any, any, any, any, any>[]) {
  return peers.filter(
    (p) =>
      p.load === this.load &&
      ioEquivalenceMatches(p.ioEquivalence, this.ioEquivalence) &&
      recordsMatch(p.paramDepIdByKey, this.paramDepIdByKey),
  );
}
public deduplicatedWith(replacement: LoadOneStep<any, any, any, any, any>): void {
  for (const attr of this.attributes) {
    replacement.attributes.add(attr);
  }
}
```

3. **`optimize`, run dependents-first.** `operation-plan.mdx:171-175`: "The optimize method is called starting with the dependencies (leaves) and working its way up the dependents (trunk) ... plans should only talk to their ancestors (and not their descendants) during optimize". Source: `this.processSteps("optimize", "dependents-first", ...)` (`OperationPlan.ts:4399-4401`). This is the phase where a descendant can ask an ancestor to inline work and then replace itself with an `access` step (`step-classes.mdx:393-415`). `finalize` then does once-only preparation and must not talk to other steps (`step-classes.mdx:464-472`); `LoadOneStep.finalize` nonetheless unions `attributes` across all kin steps with the same callback and param signature (`loadOne.ts:201-232`) so that all batches of a given loader request the same columns.

The docs stress: "Steps are ephemeral, never store a reference to a step" (`step-classes.mdx:759-776`); references must go through dep indexes because dedupe/optimize/tree-shake replace steps.

### 3.4 Abstract types: planned for every possible type, up front

`polymorphism.mdx:92-104`: "Planning a polymorphic position is a collaboration between the field's plan resolver and the abstract type's `planType` method ... The step used as the result of a polymorphic position ... represents a value we call the "specifier"." `interfaces.ts:899-909`:

```ts
export interface AbstractTypePlanner {
  /**
   * Must be a step representing the name of the object type associated with
   * the given `$specifier`, or `null` if no such type could be determined.
   */
  $__typename: Step<string | null>;
  /**
   * If not provided, will call `t.planType($specifier)`
   */
  planForType?(t: GraphQLObjectType): Step | null;
}
```

Mechanics (`OperationPlan.ts:2470-2520` `planIntoOutputPlan` polymorphic branch, `1860-2110` `mutateTodos`, `2647-2745` `polymorphicResolveType`/`polymorphicPlanObjectType`):

- `allPossibleObjectTypes` = union members or interface implementations; every one is planned ("we can't discount a type just because it doesn't have any fragments that apply to it", `OperationPlan.ts:2477-2483`).
- All `polymorphicResolveType` entries for the same abstract type in the same layer are **fanned in** first: their parent steps are converted to specifiers via `toSpecifier`/`toRecord`/raw data and a `"combined"` layer plan is created when they come from multiple layers (`OperationPlan.ts:1860-1960`; `LayerPlan.ts:187-200` "results in P*L branches rather than P^L").
- A `"polymorphic"` layer plan is created whose `parentStep` is `$__typename` (`OperationPlan.ts:2029-2042`); `planForType(t)` (or the object type's own `planType`) is called for each `t` inside it (`2043-2070`), and each type's selection set is then planned normally with `parentStep: $root` for that type (`polymorphicResolveType`, `2679-2731`).
- Types whose steps are identical share a `polymorphicPartition`; at execution each bucket carries a `polymorphicPathList` and entries whose type does not match a step's `polymorphicPaths` are flagged `FLAG_POLY_SKIPPED` and filtered out of that step's `execute` (`executeBucket.ts:626-639, 877-914`; `flow.mdx:200-219`).

So Grafast solves "lookahead cannot see through unresolved abstracts" by planning all branches statically and pruning per entry at runtime, whereas Cardinal solves it by deferring planning of the branch until the data says which types exist. Grafast's docs acknowledge the cost and mention possible future "on-demand polymorphic planning" (`polymorphism.mdx:311-326`).

### 3.5 Delivery to `execute`

`interfaces.ts:724-757`:

```ts
export interface ExecutionDetails<TDeps extends readonly [...any[]] = readonly [...any[]]> {
  /** The size of the batch being processed */
  count: number;
  /** An "execution value" for each dependency of the step */
  values: { [DepIdx in keyof TDeps]: ExecutionValue<TDeps[DepIdx]> } & { ... };
  indexMap: IndexMap;
  indexForEach: IndexForEach;
  stream: ExecutionDetailsStream | null;
  extra: ExecutionExtra;
}
```

`execute` must return a list of length `count` in batch order (`step-classes.mdx:145-152`). `values[depIdx].at(i)` reads a dependency's value for batch index `i`; unary deps expose `.unaryValue()`. The engine builds `values` by looking each dependency's `ExecutionValue` up in the bucket store (`executeBucket.ts:1044-1090`, `store.get($dep.id)`) — i.e. delivery is by **dependency index into a per-bucket store**, not by name and not by mutable bag. Everything a step needs must have been declared as a dependency at plan time.

### 3.6 Lists and arguments

Lists: a `"listItem"` layer plan is created and an internal `__ItemStep` represents "an individual item within a list" (`steps/__item.ts:8-13`); `flow.mdx:177-191`: "Grafast multiplies up the batch size, effectively flattening all the lists therein so we can handle each item in the list as an individual entry in the batch". `planListItem` (`OperationPlan.ts:2528-2585`) creates the item step, optionally wraps it via the list step's `listItem()` hook, and queues `planIntoOutputPlan` for the inner type. Non-null vs nullable object fields also change layer: a `"nullableBoundary"` layer plan is created so nulls are filtered before children run (`OperationPlan.ts:2400-2425`; `LayerPlan.ts:67-78`).

Arguments: values are never seen at plan time; `fieldArgs.getRaw(path)` / `fieldArgs.$name` return steps (`plan-resolvers/index.mdx:89-149`), which are unary (batch size 1) and typically attached with `addUnaryDependency` (`step-classes.mdx:778-804`; `LoadOneStep.setParam`, `loadOne.ts:157-164`). Arguments may also carry their own `applyPlan` resolvers that Grafast invokes after the field plan resolver returns, with the field's step as `$target` (`plan-resolvers/index.mdx:397-459`). Variables used in `@skip`/`@include` become plan **constraints**; a different plan is built per satisfying combination (`operation-plan.mdx:113-124`).

### 3.7 Dynamic-language / event-loop assumptions

- Tick-batching across `LoadOneStep`s relies on `process.nextTick`/`setTimeout(0)` to coalesce loads issued within one macrotask (`_loadCommon.ts:18-21, 232-238`).
- Duck typing everywhere: `isListCapableStep`, `stepHasToSpecifier`, `stepHasToRecord`, `$step.toSpecifier?.()`, optional lifecycle methods (`deduplicate?`), reserved method names `get`/`at`/`items`/`apply` (`step-classes.mdx:474-729`).
- Graph rewriting by identity: steps are heap objects replaced during dedupe/optimize; `stepTracker.getStepById(step.id)` must be re-resolved after each layer (`OperationPlan.ts:1668-1675`); `purgeBackTo(previousStepCount)` rolls back a failed field plan (`3118-3122`).
- Plan resolvers are synchronous by contract (`assertNotAsync`, `assertNotPromise`, `OperationPlan.ts:2296-2313`).
- `execute` may return promises per entry or a promise of the list; per-entry errors are `flagError(...)` sentinels (`step-classes.mdx:293-315`).

---

## 4. Side-by-side

| Question | Cardinal PoC [C] | graphql-ruby Next [R] | Grafast [G] |
|---|---|---|---|
| Planning pass exists? | Yes: `plan(exec_field, ctx)` per field, bottom-up (reverse DFS order) per root scope and per abstract sub-tree | No | Yes: `plan($source, fieldArgs, info)` per field, breadth-first by depth, once per operation |
| Runs when? | Per request, after tree build, before first resolve; objects empty | n/a | Per operation (cached), before any execution; no values at all |
| Registerable primitives | `preload(loader, args:, keys:)` on field or scope; `on_preload {}`; `attributes` bag; `mutable_arguments`; `planning_root`/`allows_preload?` | `extras: [:lookahead]` (top-down) | Steps + `addDependency`/`addUnaryDependency`; `deduplicate`/`deduplicatedWith`; `optimize` (dependents-first); `finalize`; `hasSideEffects` |
| Abstract types | Omitted from tree; resolved per object at runtime; sub-tree built+planned lazily; ancestors sealed (`planning_root`) | Resolved per object at runtime; per-type `SelectionsStep`; `runtime_type_at` identity map | All possible types planned up front via `planType` → `$__typename` + `planForType(t)`; fan-in (`combined`) then partition; per-entry `FLAG_POLY_SKIPPED` at runtime |
| Delivery to resolver | Manual: preload promise `.then` writes into `attributes`; resolver reads it. Lazy queue drained between generations | Manual: `dataload_all` inside `resolve_batch` parks the fiber | Positional: `details.values[depIdx].at(i)` from bucket store |
| Lists | Nested lists flattened into child scope `objects` at result-build time; planner sees no cardinality | Same flattening (`build_graphql_result`) | `listItem` layer plan + `__ItemStep`; batch size multiplies |
| Arguments | Coerced at tree build from first node; available in `plan`; per selection; loader `args:` part of batch key | Built per field step before objects; `loads:` async steps; kwargs | Steps (unary); `getRaw(path)`; `applyPlan`; constraints for `@skip`/`@include` |
| Enforcement of plan/resolve boundary | Runtime `LazySequencingError` via `:preloading`/`:executing`/`:locked` | n/a | Runtime errors (layer-plan depth check, `assertNotAsync`), `phase` string |

## 5. What would not transfer to Rust as-is

Cardinal [C]:

- `attributes` is a per-element `Hash[untyped, untyped]` written by one closure and read by another. Rust needs either a `TypeId`-keyed map (`AnyMap`-style) on field/scope, or codegen'd typed slots per field. The article's "planning notes" and the README's "resolver caches" share this bag; splitting them is a design choice.
- The preload result is *delivered by a `.then` closure mutating the field*. In Rust with `Send` futures and a generation barrier, the natural equivalent is: the executor awaits all registered loader futures for the generation, then hands each field its results by index (Grafast-style), rather than closures capturing `&mut` to the field.
- Loader identity `[loader_class, args]` used as a cache key requires `args` to be `Hash + Eq` (Ruby hashes arbitrary kwargs). Fine, but forces a trait bound on loader args.
- `LazySequencingError` state machine is a runtime check; Rust can make it a typestate: `plan(&mut PlannedField<...>)` exposes `preload`, `resolve(&ExecutingField<...>)` exposes `lazy`, and neither type has the other's method.
- Bottom-up planning mutates *ancestors* (`ancestor_scope.attributes[...] = ...`) while iterating descendants. With a tree of owned nodes this is a borrow problem; with an index-based arena it is a sequential `&mut arena[parent]` and fine. `planning_root` (skip sealed ancestors) becomes a state enum on scopes.
- `resolve_type` per object via `schema.resolve_type(abstract, object)` is dynamic dispatch on an arbitrary value; ticket 06 (abstract types without type mapping) owns that.
- Async is bolted on with fibers, per-resource semaphores and `Kernel.Sync`; sync and async loaders are executed in different code paths. In greem every loader is a future; the sync/async split disappears, but the "requeue chained lazies while others are in flight" logic (`lazy_async.rb:278-335`) is what a barrier-less scheduler would have to reproduce — out of scope for the skeleton per the map.

graphql-ruby [R]: fiber-parked steps, `Fiber[...]` globals, identity-keyed side tables on result hashes, and self-re-enqueueing steps are all Ruby-shaped; none of it is a planning primitive to carry over. Its one lesson for greem is negative: dropping the planning hook was acceptable for its compatibility goals, and batching then falls entirely on the resolver + dataloader.

Grafast [G]:

- Plan reuse across requests is the whole premise; greem plans per request (arguments and variables are concrete), so Grafast's unary-step/constraint machinery for arguments is unnecessary.
- Static planning of *all* possible types with runtime per-entry skipping is a fundamentally different bet from Cardinal's lazy sub-tree; Grafast itself flags on-demand polymorphic planning as future work. For greem's set-based model, Cardinal's approach (bucket objects by concrete type after resolution, then plan the concrete scopes) is the one already assumed by the map.
- The `optimize` dependents-first pass over a heap-allocated, identity-rewritten step DAG (`getStepById` after every layer, `purgeBackTo`) is graph surgery that would need `Rc<RefCell<..>>` or an arena with tombstones; it buys SQL-level inlining that greem is not attempting.
- `nextTick` tick-batching and promise-per-entry results depend on the JS event loop; in Rust the generation barrier is the batching point.
- Duck-typed step protocols (`get`/`at`/`items`/`toSpecifier`) map to traits, but the "optional method" pattern (`deduplicate?`, `planForType?`) would be default trait methods or separate marker traits.

The transferable ideas from Grafast are the shape of `ExecutionDetails` (positional delivery by dependency index, `count`, unary vs batch values) and the pairing of `deduplicate` + `deduplicatedWith` as the way one registration merges its requirements into a surviving peer.
