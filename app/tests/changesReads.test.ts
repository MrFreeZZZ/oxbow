import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";

function deferred() {
  let resolve!: (value: any) => void, reject!: (err: unknown) => void;
  const promise = new Promise<any>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tree = (path: string, counted = true) => ({ unstaged: [{ path }], staged: [], conflicted: [], counted });
const lists = (id: string) => ({ lists: [{ id, name: id, paths: [] }], split: [], active: null });
const turn = () => new Promise<void>(resolve => setTimeout(resolve, 0));
function setup() {
  const reads: ReturnType<typeof deferred>[] = [], counts: ReturnType<typeof deferred>[] = [], files: ReturnType<typeof deferred>[] = [];
  let listed = 0;
  const api = {
    changelists: () => { const d = deferred(); reads.push(d); return d.promise; },
    workingTree: () => { const d = deferred(); files.push(d); return d.promise; },
    workingCounts: () => { const d = deferred(); counts.push(d); return d.promise; },
    stopReads: async () => {},
  };
  const source = fs.readFileSync(new URL("../src/lib/changesReads.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const C = Function("api", "$state", "sameShown", "stopped", `${js}; return ChangesReads;`)(api, (x: unknown) => x, () => false, () => false);
  const state = new C(() => listed++, () => {});
  return { state, reads, counts, files, get listed() { return listed; } };
}

// An older changelist result, including an error, cannot replace the latest files and lists.
for (const fails of [false, true]) {
  const s = setup(), fresh = tree("fresh.txt");
  const old = s.state.load(tree("old.txt"), 1);
  const next = s.state.load(fresh, 2);
  s.reads[1].resolve(lists("new")); await next;
  if (fails) s.reads[0].reject(new Error("old failure")); else s.reads[0].resolve(lists("old"));
  await old;
  assert.equal(s.state.tree, fresh);
  assert.equal(s.state.lists.lists[0].id, "new");
  assert.equal(s.state.error, null);
  assert.equal(s.listed, 1);
}

// A slow initial file read never asks the new repository for lists of the old one.
for (const fails of [false, true]) {
  const s = setup(), fresh = tree("fresh.txt");
  const old = s.state.load(null, 1);
  const next = s.state.load(fresh, 2);
  s.reads[0].resolve(lists("new")); await next;
  if (fails) s.files[0].reject(new Error("old files failed")); else s.files[0].resolve(tree("old.txt"));
  await old;
  assert.equal(s.reads.length, 1);
  assert.equal(s.state.tree, fresh);
  assert.equal(s.state.error, null);
}

// A newer load with the same snapshot still supersedes older counts; closing drops late answers.
{
  const s = setup();
  const first = s.state.load(tree("old.txt", false), 1);
  s.reads[0].resolve(lists("old")); await first;
  const fresh = tree("fresh.txt");
  const next = s.state.load(fresh, 1);
  s.reads[1].resolve(lists("new")); await next;
  s.counts[0].resolve(tree("old.txt")); await turn();
  assert.equal(s.state.tree, fresh);
  const closing = s.state.load(tree("closed.txt"), 3);
  s.state.stop();
  s.reads[2].resolve(lists("closed")); await closing;
  assert.equal(s.state.tree, fresh);
  assert.equal(s.listed, 2);
}
