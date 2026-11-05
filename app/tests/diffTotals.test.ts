// Exercise DiffView's actual batching effect with controlled IPC and real Svelte effects.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";

const view = fs.readFileSync(new URL("../src/lib/DiffView.svelte", import.meta.url), "utf8");
const effect = view.slice(view.indexOf("  // How long each file drawn is,"), view.indexOf("  /** Starts of the blocks"));
const key = view.match(/  const openKey = .*;/)![0];
const source = `
  let items = $state.raw([]);
  let totals = $state.raw(new Map());
  let whole = $state(false);
  const shown = $derived({ files: items });
  ${key}
  ${effect}
  return { set: (files) => items = files, totals: () => totals, whole: (value) => whole = value };
`;
const compiled = compileModule(ts.transpile(`function start() { ${source} }`, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "totals.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function diff(path: string, id = path, extra = {}) {
  return { file: { path }, old: { kind: "blob", id }, hunks: [{}], ...extra };
}
function fixture() {
  const calls: { sources: unknown[]; resolve: (counts: (number | null)[]) => void; reject: (reason: unknown) => void }[] = [];
  const nav = { repo: "first" };
  let destroy!: () => void;
  const start = Function("$", "api", "nav", "onDestroy", "isFolded", "special", `${compiled}; return start;`)(runtime,
    { sourceTotals: (sources: unknown[]) => new Promise((resolve, reject) => calls.push({ sources, resolve, reject })) },
    nav, (callback: () => void) => { destroy = callback; }, (diff: any) => !!diff.folded, (diff: any) => !!diff.special);
  let state!: { set: (files: unknown[]) => void; totals: () => Map<string, number>; whole: (value: boolean) => void };
  const stop = runtime.effect_root(() => { state = start(); });
  runtime.flush();
  return { calls, nav, state, stop() { destroy(); stop(); } };
}
async function settle() { await new Promise<void>((resolve) => setImmediate(resolve)); runtime.flush(); }

test("totals use one IPC per batch, deduplicate sources and preserve per-file failures", async () => {
  const f = fixture();
  try {
    f.state.set([diff("a", "same"), diff("b", "same"), diff("bad"), diff("folded", "folded", { folded: true }), diff("special", "special", { special: true })]);
    runtime.flush();
    assert.equal(f.calls.length, 1);
    assert.equal(f.calls[0].sources.length, 2);
    f.calls[0].resolve([20, null]);
    await settle();
    assert.deepEqual([...f.state.totals().values()], [20, 20]);
    const calls = f.calls.length;
    f.state.set([diff("a", "same"), diff("b", "same")]);
    runtime.flush();
    assert.equal(f.calls.length, calls, "cached sources need no new IPC");
    f.state.set([diff("a", "same"), diff("new")]);
    runtime.flush();
    assert.deepEqual(f.calls.at(-1)!.sources, [{ kind: "blob", id: "new" }]);
    f.calls.at(-1)!.reject(new Error("read failed"));
    await settle();
    assert.equal(f.state.totals().size, 2);
  } finally { f.stop(); }
});

test("totals ignore removed files, previous repositories and a destroyed view", async () => {
  const f = fixture();
  f.state.set([diff("a"), diff("b")]);
  runtime.flush();
  f.state.set([diff("a")]);
  runtime.flush();
  f.calls[0].resolve([10, 20]);
  await settle();
  assert.deepEqual([...f.state.totals().values()], [10]);
  f.state.set([diff("c")]);
  runtime.flush();
  f.nav.repo = "second";
  f.calls.at(-1)!.resolve([30]);
  await settle();
  assert.deepEqual([...f.state.totals().values()], [10]);
  f.state.set([diff("a")]);
  runtime.flush();
  assert.equal(f.calls.length, 3, "the same path/blob in another repo has a separate key");
  f.stop();
  f.calls.at(-1)!.resolve([40]);
  await settle();
  assert.deepEqual([...f.state.totals().values()], [10]);
});
