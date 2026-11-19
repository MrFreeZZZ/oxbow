import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";

const read = (file: string) => fs.readFileSync(new URL(file, import.meta.url), "utf8");
const list = read("../src/lib/HistoryList.svelte");
const effect = list.match(/\$effect\(\(\) => \{\n    const opened = menu;[\s\S]*?\n  \}\);/)![0];
assert.ok(effect);
// Run the actual lifecycle with controlled completion order and cleanup.
const install = Function("state", "$effect", "untrack", "linkFor", "copyLink", ts.transpile(effect.replace(/\bmenu\b/g, "state.menu").replace(/\bhistory\b/g, "state.history")));
for (const mode of ["confirmed", "unknown", "failure", "closed", "other-menu", "other-history", "disposed"] as const) {
  const link = { label: "Commit Link", off: true, run: () => {} };
  const opened = { id: "abc", entries: [{ kind: "sub", label: "Copy", entries: [link] }] };
  const state = { menu: opened as typeof opened | null, history: {} };
  let finish!: (url: string | null) => void;
  let fail!: () => void;
  let cleanup: (() => void) | undefined;
  const copied: string[] = [];
  install(state, (fn: () => (() => void) | undefined) => { cleanup = fn(); }, (fn: () => unknown) => fn(),
    (id: string) => { assert.equal(id, "abc"); return new Promise<string | null>((resolve, reject) => { finish = resolve; fail = () => reject(new Error("unavailable")); }); },
    (url: string) => copied.push(url));
  assert.equal(link.off, true);
  if (mode === "closed") state.menu = null;
  if (mode === "other-menu") state.menu = { ...opened };
  if (mode === "other-history") state.history = {};
  if (mode === "disposed") cleanup?.();
  if (mode === "failure") fail();
  else finish(mode === "unknown" ? null : "https://github.com/team/repo/commit/abc");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(link.off, mode !== "confirmed", mode);
  link.run();
  assert.deepEqual(copied, mode === "confirmed" ? ["https://github.com/team/repo/commit/abc"] : [], mode);
}

// Native button keyboard activation goes through the same guarded onclick as mouse clicks.
const component = read("../src/lib/Menu.svelte");
const submenu = component.slice(component.indexOf('{#each parent.entries'));
assert.match(submenu, /class:off=\{entry.off\} aria-disabled=\{entry.off\}/);
const handler = submenu.match(/onclick=\{\(\) => ([^}]+)\}/)![1];
for (const off of [true, false]) {
  let closed = 0;
  let ran = 0;
  Function("entry", "choose", handler)({ off, run: () => ran++ }, (run: () => void) => { closed++; run(); });
  assert.equal(closed, off ? 0 : 1);
  assert.equal(ran, off ? 0 : 1);
}
