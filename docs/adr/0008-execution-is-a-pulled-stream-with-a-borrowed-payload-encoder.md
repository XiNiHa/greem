# Execution is a pulled stream with a borrowed-payload encoder

A request runs as a `Stream` (`Schema::execute_stream`, `execute_request_stream`) whose items are whatever the caller's `encode: FnMut(Payload<'_>) -> T` returns. The barrier still hands the borrowed payload to one closure while the frames are alive, as in [Inspectable retained frames preserve the borrowed payload sink](0006-inspectable-frames-preserve-borrowed-payloads.md); that closure is now the encoder. The stream drives the execution future from `poll_next`, and the run loop pauses after each shipped payload until the consumer takes it. greem never spawns, so nothing runs between polls: a slow client slows execution instead of queueing payloads, and dropping the stream drops the execution. The encoder is also where a wire format plugs in: `OwnedPayload::encode` for JSON, `greem::http::multipart_part` for a multipart part, or any serde format.

## Considered options

- A push sink bridged by an unbounded channel, as the axum example did: no backpressure, and every integration rebuilt the bridge.
- A stream of `OwnedPayload` only: backpressure, but serialization fixed to JSON in a fresh buffer, so multipart framing cost a second copy and other formats would re-encode.
- Lending the borrowed payload out of a poll method, through a lending stream or a hand-written `http_body::Body`: the frames live inside the execution future, so only a callback can see them, and hyper wants owned `Bytes` per frame anyway.
- Keeping `execute_with` beside the stream: every caller fit an encoder drained with `for_each`, and two drivers meant two code paths.
- Computing the next payload before the consumer asks: without a spawn, that work happens inside the same `poll_next` and only delays the payload already shipped.

## Consequences

`Schema` is a cheap `Clone` handle and `Operation` owns its document, so the stream is `'static` whenever the roots, context and encoder are. The encoder is synchronous; async work per payload goes after the stream. Dropping the stream mid-mutation keeps the effects of root fields that already ran; an integration that must finish a mutation drains the stream itself. HTTP response handling and non-JSON formats are [#21](https://github.com/XiNiHa/greem/issues/21) and [#22](https://github.com/XiNiHa/greem/issues/22).
