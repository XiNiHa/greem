// Audits one GraphQL-over-HTTP server with graphql-http and gates on the
// result; see docs/adr/0012-http-follows-the-graphql-over-http-draft.md.
//
//   node http-audit/audit.mjs [url] [--report <file>]
//
// The url defaults to the axum example's; --report writes the Markdown report.

import { auditServer, renderAuditResultsToHTML } from 'graphql-http';
import { readFile, writeFile } from 'node:fs/promises';
import { parseArgs } from 'node:util';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { report: { type: 'string' } },
});
const url = positionals[0] ?? 'http://127.0.0.1:8080/graphql';
const json = async (path) => JSON.parse(await readFile(new URL(path, import.meta.url), 'utf8'));
const overrides = await json('./overrides.json');
const { version } = await json('./node_modules/graphql-http/package.json');

const results = await auditServer({ url });
const byId = new Map(results.map((result) => [result.id, result]));

const problems = [];
for (const result of results) {
  if (result.status === 'error') {
    problems.push(`\`${result.id}\` ${result.name}: ${result.reason}`);
  } else if (result.status === 'warn' && !(result.id in overrides)) {
    problems.push(`\`${result.id}\` ${result.name}: ${result.reason}`);
  }
}
for (const id of Object.keys(overrides)) {
  const result = byId.get(id);
  if (!result) {
    problems.push(`\`${id}\` is overridden, but graphql-http ${version} has no such audit`);
  } else if (result.status !== 'warn') {
    problems.push(`\`${id}\` is overridden, but it is now ${result.status}: remove the override`);
  }
}

const count = (status) => results.filter((result) => result.status === status).length;
const overridden = results.filter((result) => result.status === 'warn' && result.id in overrides);
const lines = [
  `### GraphQL-over-HTTP audit ${problems.length ? 'fails' : 'passes'}`,
  '',
  `graphql-http ${version} against \`${url}\`: ${count('ok')} pass, ${overridden.length} overridden warnings, ${count('notice')} notices.`,
  '',
];
if (problems.length) {
  lines.push(...problems.map((problem) => `- ${problem}`), '');
}
if (overridden.length) {
  lines.push(
    'These SHOULD audits contradict the GraphQL-over-HTTP draft, which greem follows:',
    '',
    '| Audit | Expects | Draft |',
    '| --- | --- | --- |',
    ...overridden.map((result) => `| \`${result.id}\` | ${result.name} | ${overrides[result.id]} |`),
    '',
  );
}
lines.push(
  '<details><summary>Full graphql-http report</summary>',
  '',
  await renderAuditResultsToHTML(results),
  '</details>',
  '',
);
const report = lines.join('\n');

if (values.report) {
  await writeFile(values.report, report);
}
for (const result of results.filter((result) => result.status !== 'ok')) {
  console.log(`${result.status.padEnd(6)} ${result.id} ${result.name}: ${result.reason}`);
}
console.log(`\n${count('ok')} ok, ${count('notice')} notice, ${count('warn')} warn, ${count('error')} error`);
if (problems.length) {
  console.error(`\nThe audit fails:\n${problems.map((problem) => `- ${problem}`).join('\n')}`);
  process.exitCode = 1;
}
