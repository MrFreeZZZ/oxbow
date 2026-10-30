// Run the real session controllers with plain state and controlled backend replies. These
// tests cover ordering and failed requests without needing a WebView or a live Git repository.
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { Tickets } from "../src/lib/loads.ts";
import { Latest } from "../src/lib/latest.ts";

function controller(file: string, globals: Record<string, unknown>, result: string) {
  const source = fs.readFileSync(new URL(`../src/lib/${file}`, import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  return Function(...Object.keys(globals), `${js}; return ${result};`)(...Object.values(globals));
}
function deferred() {
  let resolve!: (value?: any) => void;
  const promise = new Promise<any>((done) => { resolve = done; });
  return { promise, resolve };
}
function session() {
  let backend: string | null = "A";
  const api = {
    closeRepo: async () => { backend = null; },
    openRepo: async (path: string) => { backend = path; return { path, name: path }; },
    goneFolder: async () => null,
  };
  class HistoryLoader {
    history: unknown = "A";
    stopPaging() {} reset() {} opened() {}
    clear() { this.history = null; }
    async first() { throw new Error("history failed"); }
  }
  const noop = { forget() {}, reset() {} };
  const C = controller("repoSession.svelte.ts", {
    $state: (v: unknown) => v, api, Tickets, HistoryLoader,
    oplog: noop, lfs: noop, activity: noop, nav: {}, github: { load() {} },
  }, "RepoSession");
  const state = new C({ leaving() {}, closed() {}, opened() {} });
  state.repo = { path: "A" };
  return { state, api, backend: () => backend };
}
{
  const { state, backend } = session();
  await state.load("B");
  assert.equal(state.repo, null);
  assert.equal(state.loader.history, null);
  assert.equal(backend(), null);
  assert.equal(state.phase, "failed");
  assert.match(state.error, /history failed/);
}
{
  const { state, backend } = session();
  await state.close();
  assert.equal(state.repo, null);
  assert.equal(backend(), null);
}
{
  const { state, backend, api } = session();
  const opening = deferred();
  const open = api.openRepo;
  api.openRepo = async (path: string) => { await opening.promise; return open(path); };
  state.loader.first = async () => ({ rows: [] });
  const first = state.load("B");
  await Promise.resolve(); await Promise.resolve();
  const closed = state.close();
  const next = state.load("C");
  opening.resolve();
  await Promise.all([first, closed, next]);
  assert.equal(state.repo.path, "C");
  assert.equal(backend(), "C");
  assert.equal(state.phase, "ready");
}

{
  const replies: ReturnType<typeof deferred>[] = [];
  const api = {
    gitSettings: async () => null, repoSettings: () => { const d = deferred(); replies.push(d); return d.promise; },
    githubAccount: async () => null, gitlabAccounts: async () => [], trustedRepos: async () => [],
  };
  const { load, model } = controller("settingsModel.svelte.ts", { $state: (v: unknown) => v, api, Latest }, "({load, model})");
  const a = load(), b = load();
  replies[1].resolve({ path: "B" }); await b;
  replies[0].resolve({ path: "A" }); await a;
  assert.equal(model.repo.path, "B");
  assert.equal(model.loading, false);
}
