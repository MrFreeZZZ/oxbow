import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { sameShown } from "../src/lib/changesView.ts";

function deferred() {
  let resolve!: (value: any) => void, reject!: (err: unknown) => void;
  const promise = new Promise<any>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tree = (path: string, counted = true) => ({ unstaged: [{ path }], staged: [], conflicted: [], counted });
const lists = (id: string) => ({ lists: [{ id, name: id, paths: [] }], split: [], active: null });
const turn = () => new Promise<void>(resolve => setTimeout(resolve, 0));
const source = fs.readFileSync(new URL("../src/lib/changesReads.svelte.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/export /g, "");
const view = fs.readFileSync(new URL("../src/lib/DiffView.svelte", import.meta.url), "utf8");
const paging = view.slice(view.indexOf("  // Files still to load while"), view.indexOf("  /** Lines one click"));
const js = ts.transpile(`${source}
  function pagingView(state) {
    let extra = $state(0);
    const pending = $derived(state.pending);
    const shown = { hidden: 0, cut: null, left: 1 };
    const PORTION = 1;
    const onMore = () => state.loadMore();
    ${paging}
    return moreFiles;
  }
`, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext });
const compiled = compileModule(js, { filename: "changesReads.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function setup() {
  const reads: ReturnType<typeof deferred>[] = [], counts: ReturnType<typeof deferred>[] = [], files: ReturnType<typeof deferred>[] = [];
  const pages: { paths: string[]; from: number; reply: ReturnType<typeof deferred> }[] = [];
  let listed = 0;
  const api = {
    changelists: () => { const d = deferred(); reads.push(d); return d.promise; },
    workingTree: () => { const d = deferred(); files.push(d); return d.promise; },
    workingCounts: () => { const d = deferred(); counts.push(d); return d.promise; },
    workingDiffPage: (paths: string[], _side: string, _whole: boolean, _reading: number | null, from: number) => {
      const reply = deferred();
      pages.push({ paths, from, reply });
      return reply.promise;
    },
    stopReads: async () => {},
  };
  const { ChangesReads, pagingView } = Function("$", "api", "sameShown", "stopped", "untrack",
    `${compiled}; return { ChangesReads, pagingView };`)(
    runtime, api, sameShown, (err: unknown) => err === "stopped", runtime.untrack);
  const state = new ChangesReads(api, () => listed++, () => {});
  return { state, pagingView, reads, counts, files, pages, get listed() { return listed; } };
}

// An older changelist result, including an error, cannot replace the latest files and lists.
for (const fails of [false, true]) {
  const s = setup(), fresh = tree("fresh.txt");
  const old = s.state.load(tree("old.txt"), 1);
  const next = s.state.load(fresh, 2);
  s.reads[1].resolve(lists("new")); await next;
  if (fails) s.reads[0].reject(new Error("old failure")); else s.reads[0].resolve(lists("old"));
  await old;
  assert.deepEqual(s.state.tree, fresh);
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
  assert.deepEqual(s.state.tree, fresh);
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
  assert.deepEqual(s.state.tree, fresh);
  const closing = s.state.load(tree("closed.txt"), 3);
  s.state.stop();
  s.reads[2].resolve(lists("closed")); await closing;
  assert.deepEqual(s.state.tree, fresh);
  assert.equal(s.listed, 2);
}

for (const staleFails of [false, true]) {
  test(`Changes retries from zero and ignores a stale page ${staleFails ? "failure" : "success"}`, async () => {
    const { state, pagingView, reads, pages } = setup();
    const tracked = ["old-a", "old-b", "new-a", "new-b"].map(path => ({ path }));
    const loaded = state.load({ ...tree(""), unstaged: tracked }, 1);
    reads[0].reject(new Error("lists failed"));
    await loaded;
    const show = (needle: string) => state.show({ kind: "all", side: "unstaged" }, false, "", { tracked }, () => needle);
    const offsets = () => pages.map(page => page.from);
    const diff = (path: string) => ({ file: { path } });
    const visible = () => state.diffs.map((d: ReturnType<typeof diff>) => d.file.path);
    const settle = async () => { await turn(); runtime.flush(); };
    let clickMore!: () => void;
    const stop = runtime.effect_root(() => { clickMore = pagingView(state); });
    try {
      show("old");
      pages[0].reply.resolve({ diffs: [diff("old-a")], next: 7, files: 2 });
      await settle();
      assert.deepEqual(offsets(), [0, 7]);

      show("new");
      assert.deepEqual(visible(), ["old-a"]);
      assert.equal(state.pending, 0);
      clickMore();
      assert.deepEqual(offsets(), [0, 7, 0]);
      pages[2].reply.reject(new Error("first page failed"));
      await settle();
      assert.equal(state.canRetry, true);
      assert.match(state.diffError, /first page failed/);
      show("new");
      clickMore();
      assert.deepEqual(offsets(), [0, 7, 0]);

      state.retry();
      state.retry();
      assert.deepEqual(offsets(), [0, 7, 0, 0]);
      assert.deepEqual(pages[3].paths, ["new-a", "new-b"]);
      pages[3].reply.resolve({ diffs: [diff("new-a")], next: 11, files: 2 });
      await settle();
      assert.deepEqual(offsets(), [0, 7, 0, 0, 11]);
      assert.deepEqual(visible(), ["new-a"]);

      if (staleFails) pages[1].reply.reject(new Error("stale page failed"));
      else pages[1].reply.resolve({ diffs: [diff("old-b")], next: null, files: 2 });
      await settle();
      clickMore();
      assert.deepEqual(offsets(), [0, 7, 0, 0, 11]);
      assert.deepEqual(visible(), ["new-a"]);
      assert.equal(state.diffError, null);
      assert.match(state.error, /lists failed/);

      pages[4].reply.resolve({ diffs: [diff("new-b")], next: null, files: 2 });
      await settle();
      assert.deepEqual(visible(), ["new-a", "new-b"]);
      assert.equal(state.pending, 0);
    } finally {
      stop();
    }
  });
}
