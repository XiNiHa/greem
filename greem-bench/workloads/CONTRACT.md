# Resolver contract

Every implementation of `schema.graphql`, in any framework and language, follows these rules so that a workload measures the executor and not the data.

## Dataset

There is no stored dataset. Every value is a function of indices, so any size is reproducible without a generator or a seed.

- `Query.users(first)` returns users `0 .. first - 1` in order.
- User `i`: `id = "i"`, `name = "user" + i`, `email = "user" + i + "@example.com"` when `i` is even, otherwise null.
- `User.posts` and `User.drafts` of user `i` are three posts `(i, 0)`, `(i, 1)`, `(i, 2)`. `posts` is a ready list; `drafts` is a lazy source (an async iterable, a stream) that yields the same items one at a time.
- Post `(i, k)`: `id = i + "-" + k`, `title = "Post " + k + " by user" + i`, `tags = ["t" + k, "u" + i]`.
- `User.friends` of user `i` are users `i + 1` and `i + 2`.
- `Query.search(size)` returns `size` items; item `j` is user `j` when `j` is even and post `(j, 0)` when `j` is odd.
- `Query.matrix(size)` returns `size` rows; row `i` holds `i` cells, cell `j` being `j` when `j` is even and null otherwise.

## Fetches

The fields that stand for a trip to a data source are `Query.users`, `Query.search`, `Query.matrix`, `User.posts`, `User.drafts` and `User.friends`. When a workload declares a fetch latency for one of them, every call of that field's resolver waits that long once, whatever the number of parent objects it serves. Leaf fields never wait.

## Idiom

Each implementation is written the way that framework's documentation would have a user write it, not tuned for the benchmark: whatever the framework offers for batching, the implementation uses it as documented and nothing more.

## Workload files

Each workload is a directory holding:

- `document.graphql`: the operation. It declares a `$size: Int!` variable that the consumer binds to each entry of `sizes`.
- `workload.json`:
  - `sizes`: the values of `$size` to run at, ascending. The first is the size the checked-in expected output was produced at.
  - `variables`: other variables, if any.
  - `latencyMs`: fetch latency in milliseconds by `Type.field`, if any.
  - `requires`: capabilities a framework must have to run it: `defer`, `stream`.
- `expected.json`: the output at the smallest size as greem produces it: a single response object, or the list of payloads in order for an incremental workload. Compare structurally after parsing, never by bytes.
