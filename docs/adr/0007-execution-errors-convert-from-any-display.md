# `greem::Error` converts from anything `Display` and is not a std error

`greem::Error` is the message and extensions of an execution error as the response will show it, not a link in a chain of causes. Resolvers mostly call service code that returns its own errors, often `anyhow::Error` or a boxed `dyn Error`, so `greem::Error` implements `From<E: Display>` and `?` works on any of them. Coherence then forbids `greem::Error` from implementing `Display` or `std::error::Error`, because either would overlap with the reflexive `From<T> for T`; callers read it with `message()`, `extensions()` or `Debug`. async-graphql's `Error` makes the same trade.

## Considered options

- Implement `std::error::Error` and drop the blanket conversion: `greem::Error` composes with other error types, but every resolver call site needs `.map_err(...)`. Nothing in the workspace needed `greem::Error` as a std error.
- Keep the narrower `From<E: std::error::Error>` with `Display`: `?` still fails on `anyhow::Error`, `Box<dyn Error + Send + Sync>` and `String`, none of which implement `std::error::Error`.

## Consequences

The conversion keeps only `to_string()`: the source chain is dropped, and whatever the source error prints reaches the client. Resolvers that must hide internal detail build the error with `Error::new`.
