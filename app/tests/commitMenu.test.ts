import assert from "node:assert/strict";
import fs from "node:fs";
import { mock, test } from "node:test";
import ts from "typescript";
import type { CommitMenuApp } from "../src/lib/commitMenu.ts";
import type { Request } from "../src/lib/confirm.svelte.ts";
import type { PatchTarget } from "../src/lib/patches.ts";
import type { History, HistoryRow } from "../src/lib/types.ts";
import type { MenuEntry } from "../src/lib/Menu.svelte";
import { compareHead, compareLabel, compareRef } from "../src/lib/compareRefs.ts";
import { ancestors } from "../src/lib/historyAncestry.ts";
import { dropsPushed, lockedCommit, lockedNote, protectedHead } from "../src/lib/protect.ts";

// Load the menu without its Tauri/Svelte imports, keeping the actual menu construction.
const source = fs.readFileSync(new URL("../src/lib/commitMenu.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const saveRequest = Promise.resolve<Request>({ title: "Save patches", body: [], icon: "patch", button: "Save Patches" });
const savePatchesRequest = mock.fn((_target: PatchTarget) => saveRequest);
const globals = {
  menuIcons: {}, ancestors, dropsPushed, lockedCommit, lockedNote, protectedHead,
  resetModes: [], compareHead, compareLabel, compareRef, savePatchesRequest,
  keys: (key: string) => key,
  github: { gitlab: [{ remote: "origin" }], mergeRequestOf: () => null },
  gitlabOf: (h: History, projects: { remote: string }[]) => projects.find((p) => p.remote === h.defaultRemote),
};
const menu = Function(...Object.keys(globals), `${ts.transpile(source)}; return commitMenu;`)(...Object.values(globals)) as (row: HistoryRow, app: CommitMenuApp) => MenuEntry[];
const row = { id: "abc1234", summary: "Commit", parents: [], labels: [], graph: { color: 1, branch: "topic" }, worktree: null, unpushed: false } as unknown as HistoryRow;
const history = {
  head: { branch: "topic", commit: row.id }, trunk: "main", trunkRef: null,
  operation: null, bisect: null, stashes: [], refs: [], defaultRemote: null,
  rows: [row], protected: [], remotes: [],
} as unknown as History;
const merging = { ...history, operation: { kind: "merge" } } as History;
let applied = 0;
const applyPatches = () => { applied++; };
function entries(graph = row.graph, h = history, overrides: Partial<CommitMenuApp> = {}, target = row) {
  return menu({ ...target, graph }, {
    history: h, ctx: { history: h }, selectedRow: null, headStack: new Set(), applyPatches,
    patchTarget: (r: HistoryRow): PatchTarget => ({ to: r.id, count: 1, since: null, name: r.summary, what: [] }),
    ...overrides,
  } as unknown as CommitMenuApp);
}
function labels(items: MenuEntry[]) {
  return items.map((entry) => entry.kind === "sep" ? null : entry.label);
}
function patches(items = entries()) {
  const submenu = items.find((e) => e.kind === "sub" && e.label === "Patches");
  assert.ok(submenu?.kind === "sub");
  return submenu.entries;
}
function apply(graph = row.graph, h = history) {
  return patches(entries(graph, h))[1];
}

test("the ordinary branch tip has four groups with Drop last in the first group", () => {
  const tip = {
    ...row, parents: ["parent"], labels: [{ name: "topic", kind: "local", color: 1, head: true }],
  } satisfies HistoryRow;
  const h: History = {
    ...history, trunkRef: "refs/heads/main", defaultRemote: "origin", remotes: ["origin"], rows: [tip],
    refs: [{ name: "topic", kind: "local", target: tip.id, remote: null, tracking: null }],
  };
  const items = entries(tip.graph, h, { headStack: new Set([tip.id]) }, tip);
  assert.deepEqual(labels(items.slice(1)), [
    "Undo Commit", "Edit Message…", "Split Commit…", "Edit Stack…", "Drop Commit…",
    null,
    "Revert Commit…", "Fix a Mistake…", "Find What Broke This…",
    null,
    "Create Merge Request for topic…", "New Branch from Here…", "New Tag Here…",
    null,
    "Browse Files at This Commit", "Compare", "Patches", "Copy",
  ]);
  assert.deepEqual(items.slice(-3).map((entry) => entry.kind), ["sub", "sub", "sub"]);
});

test("Checkout, Merge and Rebase share the optional top group and respect branch protection", () => {
  const older = {
    ...row, id: "parent", labels: [{ name: "other", kind: "local", color: 2, head: false }],
  } satisfies HistoryRow;
  const h = { ...history, rows: [{ ...row, parents: [older.id] }, older], remotes: ["origin"] };
  const items = entries(older.graph, h, {}, older);
  assert.deepEqual(labels(items.slice(1, 6)), [
    "Check Out other", "Check Out This Commit…", "Merge other into topic…", "Rebase topic onto other…", null,
  ]);
  assert.ok(items.some((entry) => entry.kind === "sub" && entry.label === "Reset topic to Here"));

  const protectedItems = entries(older.graph, { ...h, protected: ["topic"] }, {}, older);
  assert.deepEqual(labels(protectedItems.slice(1, 5)), [
    "Check Out other", "Check Out This Commit…", "Merge other into topic…", null,
  ]);
  assert.ok(!labels(protectedItems).some((label) => label?.startsWith("Reset ") || label?.startsWith("Rebase ")));
  assert.ok(protectedItems.some((entry) => entry.kind === "note" && entry.label ===
    "topic is protected: its pushed commits can be reverted, not rewritten."));
  assert.ok(labels(protectedItems).includes("New Branch from Here…"));

  const busy = labels(entries(older.graph, { ...h, operation: merging.operation }, {}, older));
  assert.deepEqual(busy.slice(1, 4), ["Check Out other", "Check Out This Commit…", null]);
  assert.ok(busy.includes("New Branch from Here…"));
  const detached = { ...history, head: { ...history.head, branch: null } };
  assert.ok(labels(entries(row.graph, detached)).includes("Create Branch…"));
});

assert.deepEqual(patches().map((entry) => entry.label), ["Save as Patch…", "Apply Patches…"]);
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
  [{ ...row.graph, branch: "other" }, merging],
] as const) {
  const entry = apply(graph, h);
  assert.equal(entry.off, true);
  assert.equal(entry.detail, "Switch to this branch to apply patches.");
}
const main = { ...history, head: { ...history.head, branch: "main" } };
assert.equal(apply({ ...row.graph, color: 0, branch: null }, main).off, false);
assert.equal(apply({ ...row.graph, color: 0, branch: "other" }, main).off, true);
const applyingDuringMerge = apply(row.graph, merging);
assert.equal(applyingDuringMerge.off, true);
assert.equal(applyingDuringMerge.detail, "Finish or abort what git is in the middle of first.");

test("Save for a range forwards the original patch target to the export handler", () => {
  const target: PatchTarget = { to: row.id, since: "base", count: 3, name: row.summary, what: [] };
  const run = mock.fn<CommitMenuApp["run"]>();
  const [save] = patches(entries(row.graph, history, { patchTarget: () => target, run }));
  assert.equal(save.label, "Save 3 Commits as Patches…");
  assert.ok(!save.off);
  save.run();
  assert.equal(savePatchesRequest.mock.callCount(), 1);
  assert.equal(savePatchesRequest.mock.calls[0].arguments[0], target);
  assert.equal(run.mock.callCount(), 1);
  assert.equal(run.mock.calls[0].arguments[0], saveRequest);
});

for (const found of [null, row.id]) {
  const action = found ? "Finish Bisect…" : "Stop Bisect…";
  test(`Patches stays before Copy in the ${action} menu`, () => {
    const h = {
      ...history, head: { ...history.head, branch: null },
      bisect: { found, start: "topic", startIsBranch: true },
    } as History;
    const items = entries(row.graph, h);
    assert.ok(labels(items).includes(action));
    assert.deepEqual(labels(items.slice(-4)), ["Browse Files at This Commit", "Compare", "Patches", "Copy"]);
    const [save, apply] = patches(items);
    assert.equal(save.label, "Save as Patch…");
    assert.ok(!save.off);
    assert.equal(apply.label, "Apply Patches…");
    assert.equal(apply.off, true);
    assert.equal(apply.detail, "Finish the bisect first.");
  });
}

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
