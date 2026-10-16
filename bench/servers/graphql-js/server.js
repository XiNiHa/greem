// The graphql-js implementation of the resolver contract
// (../../workloads/CONTRACT.md): `buildSchema` over the shared SDL, plain
// objects whose fetch fields are functions for the default resolver, and
// graphql-http, the server graphql-js's docs point to, on `node:http`.
// graphql-http has no incremental delivery, so a request accepting
// `multipart/mixed` runs graphql-js's own `experimentalExecuteIncrementally`
// behind graphql-http's request parser, with the multipart framing written
// here.

import { createServer } from "node:http";
import { createHandler, parseRequestParams } from "graphql-http/lib/use/http";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
  GraphQLDeferDirective,
  GraphQLError,
  GraphQLSchema,
  GraphQLStreamDirective,
  buildSchema,
  experimentalExecuteIncrementally,
  parse,
  validate,
} from "graphql";

const workloadsDir = process.env.WORKLOADS ?? new URL("../../workloads", import.meta.url).pathname;
const port = Number(process.env.PORT ?? 4003);
const POSTS_PER_USER = 3;

// `buildSchema` leaves out `@defer` and `@stream`, which graphql-js 17 still
// treats as opt-in, so they are added here.
const built = buildSchema(readFileSync(join(workloadsDir, "schema.graphql"), "utf8"));
const schema = new GraphQLSchema({
  ...built.toConfig(),
  directives: [...built.getDirectives(), GraphQLDeferDirective, GraphQLStreamDirective],
});

/** Every workload's fetch latency in milliseconds by name. */
const latencies = new Map(
  readdirSync(workloadsDir)
    .filter((name) => statSync(join(workloadsDir, name)).isDirectory())
    .map((name) => {
      const manifest = JSON.parse(readFileSync(join(workloadsDir, name, "workload.json"), "utf8"));
      return [name, manifest.latencyMs ?? {}];
    }),
);

function latencyFor(headers) {
  return latencies.get(headers["x-workload"]) ?? {};
}

async function fetch(context, field) {
  const ms = context.latency[field];
  if (ms) await sleep(ms);
}

// Derived fields are getters so only the selected ones are computed.
function post(owner, index) {
  return {
    __typename: "Post",
    get id() {
      return `${owner}-${index}`;
    },
    get title() {
      return `Post ${index} by user${owner}`;
    },
    get tags() {
      return [`t${index}`, `u${owner}`];
    },
  };
}

function postsOf(owner) {
  return Array.from({ length: POSTS_PER_USER }, (_, index) => post(owner, index));
}

function user(id) {
  return {
    __typename: "User",
    get id() {
      return String(id);
    },
    get name() {
      return `user${id}`;
    },
    get email() {
      return id % 2 === 0 ? `user${id}@example.com` : null;
    },
    async posts(_args, context) {
      await fetch(context, "User.posts");
      return postsOf(id);
    },
    async *drafts(_args, context) {
      await fetch(context, "User.drafts");
      for (let index = 0; index < POSTS_PER_USER; index++) yield post(id, index);
    },
    async friends(_args, context) {
      await fetch(context, "User.friends");
      return [user(id + 1), user(id + 2)];
    },
  };
}

const rootValue = {
  async users({ first }, context) {
    await fetch(context, "Query.users");
    return Array.from({ length: Math.max(first, 0) }, (_, id) => user(id));
  },
  async search({ size }, context) {
    await fetch(context, "Query.search");
    return Array.from({ length: Math.max(size, 0) }, (_, j) =>
      j % 2 === 0 ? user(j) : post(j, 0),
    );
  },
  async matrix({ size }, context) {
    await fetch(context, "Query.matrix");
    return Array.from({ length: Math.max(size, 0) }, (_, i) =>
      Array.from({ length: i }, (_, j) => (j % 2 === 0 ? j : null)),
    );
  },
};

const BOUNDARY = "-";
const MULTIPART = `multipart/mixed; boundary="${BOUNDARY}"; incrementalSpec=v0.2`;

function part(payload) {
  const json = JSON.stringify(payload);
  const after = payload.hasNext ? `\r\n--${BOUNDARY}\r\n` : `\r\n--${BOUNDARY}--\r\n`;
  return `Content-Type: application/json; charset=utf-8\r\n\r\n${json}${after}`;
}

function json(res, status, body) {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}

// graphql-js's plain `execute` refuses a schema carrying `@defer` and
// `@stream`; the incremental entry point returns an ordinary result for an
// ordinary document.
const single = createHandler({
  schema,
  rootValue,
  context: (req) => ({ latency: latencyFor(req.raw.headers) }),
  execute: async (args) => {
    const result = await experimentalExecuteIncrementally(args);
    if ("initialResult" in result) {
      return { errors: [new GraphQLError("incremental delivery needs Accept: multipart/mixed")] };
    }
    return result;
  },
});

async function incremental(req, res) {
  const params = await parseRequestParams(req, res);
  if (!params) return;
  let document;
  try {
    document = parse(params.query);
  } catch (e) {
    return json(res, 400, { errors: [e] });
  }
  const errors = validate(schema, document);
  if (errors.length > 0) return json(res, 400, { errors });
  const result = await experimentalExecuteIncrementally({
    schema,
    document,
    rootValue,
    contextValue: { latency: latencyFor(req.headers) },
    variableValues: params.variables,
    operationName: params.operationName,
  });
  if (!("initialResult" in result)) return json(res, 200, result);
  res.writeHead(200, { "content-type": MULTIPART });
  res.write(`\r\n--${BOUNDARY}\r\n${part(result.initialResult)}`);
  for await (const payload of result.subsequentResults) res.write(part(payload));
  res.end();
}

createServer((req, res) => {
  const handle = (req.headers.accept ?? "").includes("multipart/mixed") ? incremental : single;
  handle(req, res).catch((e) => {
    if (!res.headersSent) json(res, 500, { errors: [{ message: e.message }] });
    else res.end();
  });
}).listen(port, "127.0.0.1", () => {
  console.log(`graphql-js bench server listening on http://127.0.0.1:${port}/graphql`);
});
