// Drives one GraphQL-over-HTTP server with the graphql-sse reference client
// in distinct connections mode and checks that its parser accepts greem's
// event framing; see docs/adr/0014-sse-only-clients-get-request-errors-as-events.md.
//
//   node http-audit/sse.mjs [url]
//
// The url defaults to the axum example's.

import { createClient } from "graphql-sse";
import { deepStrictEqual, ok } from "node:assert/strict";

const url = process.argv[2] ?? "http://127.0.0.1:8080/graphql";
const client = createClient({ url, singleConnection: false, retryAttempts: 0 });

/** Every `next` result of one operation, once `complete` arrives. */
const run = (query) =>
  new Promise((resolve, reject) => {
    const results = [];
    client.subscribe(
      { query },
      {
        next: (result) => results.push(result),
        error: reject,
        complete: () => resolve(results),
      },
    );
  });

const checks = {
  async "a query is one next event, then complete"() {
    deepStrictEqual(await run("{ users { name } }"), [
      { data: { users: [{ name: "Ann" }, { name: "Bob" }] } },
    ]);
  },
  async "a subscription is one next event per event"() {
    deepStrictEqual(await run("subscription { postAdded { title } }"), [
      { data: { postAdded: { title: "Hello" } } },
      { data: { postAdded: { title: "Again" } } },
      { data: { postAdded: { title: "Hi" } } },
    ]);
  },
  async "incremental delivery is one next event per payload"() {
    const payloads = await run("{ users { name ... @defer { posts { title } } } }");
    ok(payloads.length >= 2, `expected several payloads, got ${payloads.length}`);
    deepStrictEqual(payloads[0].hasNext, true);
    deepStrictEqual(payloads.at(-1).hasNext, false);
    deepStrictEqual(payloads[0].data.users[0], { name: "Ann" });
  },
  async "a validation error arrives as a next event, not a failed connection"() {
    const results = await run("{ nope }");
    deepStrictEqual(results.length, 1);
    ok(results[0].data === undefined, `unexpected data: ${JSON.stringify(results[0])}`);
    ok(
      typeof results[0].errors?.[0]?.message === "string",
      `no error message: ${JSON.stringify(results[0])}`,
    );
  },
};

let failed = 0;
for (const [name, check] of Object.entries(checks)) {
  try {
    await check();
    console.log(`ok   ${name}`);
  } catch (e) {
    failed += 1;
    console.log(`FAIL ${name}\n     ${e?.message ?? e}`);
  }
}
client.dispose();
if (failed) {
  console.log(`\n${failed} graphql-sse check(s) failed against ${url}`);
  process.exit(1);
}
console.log(
  `\ngraphql-sse ${(await import("graphql-sse/package.json", { with: { type: "json" } })).default.version} accepts every frame from ${url}`,
);
