# `greem::http` follows the GraphQL-over-HTTP draft, not graphql-http's audits

`greem::http::handle` sets status codes and media types the way the GraphQL-over-HTTP draft does (`spec/GraphQLOverHTTP.md` in graphql/graphql-over-http, as of September 2026):

- 400 for a body or document that does not parse;
- 422 for any other request error, and for parameters that are not well-formed;
- 294 when `data` comes with `errors`;
- 405 for a mutation over GET;
- 406 when `Accept` names nothing greem can send, and 415 for a body that is not JSON.

A legacy client that accepts only `application/json` gets the same codes, and only a `2xx` keeps that media type. graphql-http, whose server audits CI runs, lags the draft: version 1.23.1 still expects 200 for request errors under `application/json`, 400 for variable coercion failures under `application/graphql-response+json`, and 400 for malformed parameters. Where the two disagree, greem follows the draft. `http-audit/overrides.json` names each SHOULD audit that disagrees and cites the draft section it contradicts, so the gate still fails on any other warning, and on an override that starts passing.

## Considered options

- Follow graphql-http's audits: the gate would be zero warnings with no list to keep, but greem would contradict the current draft, and under `application/json` a request error would look like success to anything that reads only the status.
- A configurable status policy: every integration would have to choose one, and the audit could check only one of them.
- 200 instead of 294 for partial success: 294 is not registered with IANA. The draft recommends it anyway, since a client that does not know a `2xx` code must treat it as 200.

## Consequences

Request errors carry a `RequestErrorKind` from the step that raised them, and the status code is derived from it. A multipart response is always 200, because its status goes out before the later payloads exist. A missing `Accept` header, and `*/*`, get `application/json`; the draft leaves that choice to the server, and it keeps legacy clients working. Intermediaries that allow only known status codes may need to be told about 294. When graphql-http catches up with the draft, its overrides become passing audits and the gate asks for them to be removed. ADR-0014 carves out the one exception: a client that accepts only `text/event-stream` gets request errors as events, as graphql-sse requires.
