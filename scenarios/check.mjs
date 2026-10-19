// The scenario check (docs/e2e.md Section 7, J8): which of the scenarios Section 5 names are built,
// and whether they pass. It reads the ids from the document's tables, the Rust scenarios' libtest
// lines and Playwright's JSON report, matches them, and writes the badge and a summary. A test that
// names no scenario in the document fails the check, and so does a built scenario that fails.
//
//   node scenarios/check.mjs --doc docs/e2e.md --rust <cargo test output> [--web <report.json>]
//                            [--badge <e2e.json>] [--summary <markdown file>]
import fs from "node:fs";

const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : null; };
const doc = fs.readFileSync(arg("--doc") || "docs/e2e.md", "utf8");

// Section 5's tables: a row whose first cell is a code id, `A1`.
const section = doc.slice(doc.indexOf("## 5. The scenarios"), doc.indexOf("\n## 6."));
const specified = [...section.matchAll(/^\| `([A-Z]\d+)` \| \*\*([^*]+)\*\*/gm)].map((m) => ({ id: m[1], title: m[2].replace(/\.$/, "") }));
if (!specified.length) { console.error("no scenarios found in Section 5"); process.exit(2); }
const known = new Set(specified.map((s) => s.id));

// Results by scenario id: every test that names one, and how it ended.
const results = new Map();
const unknown = [];
const note = (id, test, outcome) => {
  if (!known.has(id)) { unknown.push(`${test} names ${id}, which Section 5 does not`); return; }
  (results.get(id) || results.set(id, []).get(id)).push({ test, outcome });
};
const rust = arg("--rust");
if (rust && fs.existsSync(rust)) {
  for (const m of fs.readFileSync(rust, "utf8").matchAll(/^test (?:\S+::)?(([a-z])(\d+)_\S*) \.\.\. (ok|FAILED|ignored)/gm)) {
    note(m[2].toUpperCase() + m[3], m[1], m[4] === "ok" ? "passed" : m[4] === "ignored" ? "skipped" : "failed");
  }
}
const web = arg("--web");
if (web && fs.existsSync(web)) {
  const walk = (suite) => {
    for (const spec of suite.specs || []) {
      const m = /^([A-Z]\d+)\b/.exec(spec.title);
      if (!m) { unknown.push(`"${spec.title}" names no scenario`); continue; }
      const outcome = spec.tests.every((t) => t.results.at(-1)?.status === "passed") ? "passed"
        : spec.tests.every((t) => t.results.at(-1)?.status === "skipped") ? "skipped" : "failed";
      note(m[1], spec.title, outcome);
    }
    for (const s of suite.suites || []) walk(s);
  };
  for (const s of JSON.parse(fs.readFileSync(web, "utf8")).suites || []) walk(s);
}

const rows = specified.map(({ id, title }) => {
  const rs = results.get(id) || [];
  const state = !rs.length || rs.every((r) => r.outcome === "skipped") ? "unbuilt"
    : rs.some((r) => r.outcome === "failed") ? "failed" : "passed";
  return { id, title, state, tests: rs.map((r) => r.test) };
});
const passed = rows.filter((r) => r.state === "passed").length;
const failed = rows.filter((r) => r.state === "failed").length;
const color = failed || unknown.length ? "red" : passed === rows.length ? "brightgreen" : "e0a24a";

const badge = { schemaVersion: 1, label: "e2e", message: `${passed}/${rows.length}`, color };
if (arg("--badge")) fs.writeFileSync(arg("--badge"), JSON.stringify(badge) + "\n");
const mark = { passed: "passed", failed: "FAILED", unbuilt: "not built" };
const summary = [
  `### End-to-end scenarios: ${passed} of ${rows.length} passing`, "",
  "| Scenario | | Result | Tests |", "|---|---|---|---|",
  ...rows.map((r) => `| ${r.id} | ${r.title} | ${mark[r.state]} | ${r.tests.join(", ") || "-"} |`),
  ...(unknown.length ? ["", "**Tests that name no scenario in docs/e2e.md:**", ...unknown.map((u) => `- ${u}`)] : []),
].join("\n") + "\n";
if (arg("--summary")) fs.appendFileSync(arg("--summary"), summary);
process.stdout.write(summary);
process.exit(failed || unknown.length ? 1 : 0);
