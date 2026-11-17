import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import type { CommitMenuApp } from "../src/lib/commitMenu.ts";
import type { History, HistoryRow } from "../src/lib/types.ts";
import type { MenuEntry } from "../src/lib/Menu.svelte";
import { compareHead, compareLabel, compareRef } from "../src/lib/compareRefs.ts";

// Load the menu without its Tauri/Svelte imports, keeping the actual menu construction.
const source = fs.readFileSync(new URL("../src/lib/commitMenu.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const globals = {
  menuIcons: {}, ancestors: () => new Set(), dropsPushed: () => false,
  protectedHead: () => null, resetModes: [], compareHead, compareLabel, compareRef,
};
const menu = Function(...Object.keys(globals), `${ts.transpile(source)}; return commitMenu;`)(...Object.values(globals)) as (row: HistoryRow, app: CommitMenuApp) => MenuEntry[];
const row = { id: "abc1234", summary: "Commit", parents: [], labels: [], graph: { color: 1, branch: "topic" }, worktree: null } as unknown as HistoryRow;
const history = { head: { branch: "topic", commit: row.id }, trunk: "main", trunkRef: null, operation: null, bisect: null, stashes: [], refs: [], defaultRemote: null } as unknown as History;
let applied = 0;
const applyPatches = () => { applied++; };
function entries(graph = row.graph, h = history, overrides: Partial<CommitMenuApp> = {}, target = row) {
  return menu({ ...target, graph }, {
    history: h, ctx: { history: h }, selectedRow: null, headStack: new Set(), applyPatches,
    patchTarget: () => ({ count: 1, since: null }),
    ...overrides,
  } as unknown as CommitMenuApp);
}
function apply(graph = row.graph, h = history) {
  const entry = entries(graph, h).find((e) => e.kind === "item" && e.label === "Apply Patches…");
  assert.ok(entry?.kind === "item");
  return entry;
}
const items = entries();
const index = items.findIndex((e) => e.kind === "item" && e.label === "Save as Patch…");
assert.equal(items[index + 1].kind, "item");
assert.equal(apply().off, false);
assert.equal(apply().detail, undefined);
assert.equal(apply().run, applyPatches, "use the window's existing patch flow");
apply().run();
assert.equal(applied, 1);
for (const [graph, h] of [
  [{ ...row.graph, branch: "other" }, history],
  [{ ...row.graph, branch: null }, history],
  [row.graph, { ...history, head: { ...history.head, branch: null } }],
  [{ ...row.graph, color: 0, branch: null }, { ...history, trunk: null }],
] as const) {
  const entry = apply(graph, h);
  assert.equal(entry.off, true);
  assert.equal(entry.detail, "Switch to this branch to apply patches.");
}
const main = { ...history, head: { ...history.head, branch: "main" } };
assert.equal(apply({ ...row.graph, color: 0, branch: null }, main).off, false);
assert.equal(apply({ ...row.graph, color: 0, branch: "other" }, main).off, true);
assert.equal(apply(row.graph, { ...history, operation: { kind: "merge" } } as History).off, true);

// Exercise the existing App flow: picker -> preview request -> run, with cancel/stale guards.
const appSource = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");
assert.match(appSource, /return commitMenuOf\(row, \{[^\n]*patchTarget, applyPatches, openPull/);
assert.match(appSource, /return commitMenuOf\(row, \{[^\n]*selectedRow/);
const flow = appSource.match(/async function applyPatches\(\) \{[\s\S]*?\n  \}/)![0];
for (const mode of ["apply", "cancel", "stale"] as const) {
  const ctx = {};
  const paths = ["/tmp/example.patch"];
  const calls: unknown[] = [];
  const state = { ctx };
  const runFlow = Function("state", "choosePatchFiles", "applyPatchesRequest", "run", `${flow.replace(/branchCtx/g, "state.ctx")}; return applyPatches;`)(
    state,
    async () => { if (mode === "stale") state.ctx = {}; return mode === "cancel" ? null : paths; },
    (c: unknown, p: unknown) => { calls.push([c, p]); return "preview request"; },
    (request: unknown) => { calls.push(request); },
  );
  await runFlow();
  assert.deepEqual(calls, mode === "apply" ? [[ctx, paths], "preview request"] : []);
}

// Both normal and bisect Copy menus start unverified, after Message.
for (const h of [history, { ...history, bisect: { found: true, start: "topic", startIsBranch: false } } as History]) {
  const copy = entries(row.graph, h).find((entry) => entry.kind === "sub" && entry.label === "Copy");
  assert.ok(copy?.kind === "sub");
  assert.deepEqual(copy.entries.map((entry) => entry.label), ["SHA", "Short SHA", "Message", "Commit Link"]);
  assert.equal(copy.entries[3].off, true);
}

// Selection is the base, right-clicked commit the target, even with colliding ref names.
const base = { ...row, id: "1".repeat(40), labels: [{ kind: "tag", name: "feature" }] } as HistoryRow;
const target = { ...row, id: "2".repeat(40), labels: [{ kind: "local", name: "feature" }, { kind: "tag", name: "feature" }] } as HistoryRow;
for (const h of [
  history,
  { ...history, trunkRef: "refs/heads/main" },
  { ...history, bisect: { found: true, start: "topic", startIsBranch: false } },
  { ...history, trunkRef: "refs/heads/main", bisect: { found: true, start: "topic", startIsBranch: true } },
] as History[]) {
  const calls: string[][] = [];
  const overrides = { selectedRow: base, openCompare: (from: string, to: string) => { calls.push([from, to]); } };
  const compare = entries(target.graph, h, overrides, target).find((e) => e.kind === "sub" && e.label === "Compare");
  assert.ok(compare?.kind === "sub");
  const selected = compare.entries.at(-1)!;
  assert.equal(selected.label, "With Selected Commit…");
  assert.equal(selected.hint, base.id.slice(0, 7));
  selected.run();
  assert.deepEqual(calls.pop(), [base.id, target.id]);
  // Existing branch comparisons retain their full ref target and order.
  const previous = [
    ...(!h.bisect || h.bisect.startIsBranch ? ["refs/heads/topic"] : []),
    ...(h.trunkRef ? [h.trunkRef] : []),
  ];
  assert.deepEqual(compare.entries.slice(0, -1).map((e) => e.label), previous.map((ref) => `With ${compareLabel(ref)}`));
  for (const entry of compare.entries.slice(0, -1)) entry.run();
  assert.deepEqual(calls, previous.map((ref) => [ref, "refs/heads/feature"]));
  for (const selectedRow of [null, { ...base, worktree: { staged: 0, unstaged: 1 } }, { ...target }]) {
    const invalid = entries(target.graph, h, { selectedRow } as Partial<CommitMenuApp>, target).find((e) => e.kind === "sub" && e.label === "Compare");
    if (invalid?.kind === "sub") assert.ok(invalid.entries.every((e) => e.label !== "With Selected Commit…"));
    else assert.equal(previous.length, 0, "hide empty Compare submenu");
  }
}
