import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import type * as Drafts from "../src/lib/commitDrafts.svelte.ts";
import { test } from "node:test";
import { api, apiForSession, type RepoNotice, type RepoTabOpen } from "../src/lib/api.ts";
import { repoTabActions, RepoTabs, type OpenFrom, type OpenTab } from "../src/lib/repoTabs.ts";
import type { History, RepoSummary } from "../src/lib/types.ts";

const summary = (id: number): RepoSummary => ({ path: `/repo/${id}`, name: String(id), restricted: [] });
const history = {} as History;
const opened = (session: number): RepoTabOpen => ({ kind: "opened", session, repo: summary(session) });

function windowTabs(backend: ConstructorParameters<typeof RepoTabs>[0], beforeClose = async (_id: number) => true) {
  const state = { tabs: [] as readonly OpenTab[], active: null as number | null, free: true };
  const errors: unknown[] = [];
  const notices: RepoNotice[] = [];
  const tabs = new RepoTabs(backend, {
    show: (tabs, active) => Object.assign(state, { tabs, active }),
    canSwitch: () => state.free,
    beforeClose,
    notice: (notice) => notices.push(notice),
  });
  const from: OpenFrom = {
    live: () => true,
    trust: async () => null,
    reopened: () => {},
    fail: async (_path, error) => { errors.push(error); },
  };
  return { tabs, state, from, errors, notices };
}

test("repository shortcuts wrap in tab order and close picks the right neighbor, then the left", async () => {
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: async () => {},
    close: async () => {},
  });
  for (const id of ["1", "2", "3", "4"]) await tabs.open(id, from);
  await tabs.step(1);
  assert.equal(state.active, 1);
  await tabs.step(-1);
  assert.equal(state.active, 4);
  await tabs.activate(2);
  await tabs.close(2);
  assert.equal(state.active, 3);
  await tabs.close(4);
  assert.equal(state.active, 3);
  await tabs.close(3);
  assert.equal(state.active, 1);
  await tabs.close(1);
  assert.equal(state.active, null);
});

test("closing the other tabs checks all guards before selecting the target and closing anything", async () => {
  const events: string[] = [];
  let blocked = 3;
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: async (id) => { events.push(`activate:${id}`); },
    close: async (id) => { events.push(`close:${id}`); },
  }, async (id) => {
    events.push(`guard:${id}`);
    return id !== blocked;
  });
  for (const id of ["1", "2", "3"]) await tabs.open(id, from);
  events.length = 0;
  await tabs.closeOthers(2);
  assert.deepEqual(events, ["guard:1", "guard:3"]);
  assert.deepEqual(state.tabs.map(tab => tab.id), [1, 2, 3]);
  assert.equal(state.active, 3);

  blocked = 0;
  events.length = 0;
  await tabs.closeOthers(2);
  assert.deepEqual(events, ["guard:1", "guard:3", "activate:2", "close:1", "close:3"]);
  assert.deepEqual(state.tabs.map(tab => tab.id), [2]);
  assert.equal(state.active, 2);
});

test("the tab context menu targets the tab that opened it", () => {
  const calls: unknown[] = [];
  const target = { id: 2, repo: summary(2), history };
  const actions = {
    copyPath: (path: string) => calls.push(["copy", path]),
    close: (id: number) => calls.push(["close", id]),
    closeOthers: (id: number) => calls.push(["keep", id]),
  };
  const menu = repoTabActions(target, 3, actions);
  menu.copy.run();
  menu.close.run();
  menu.others.run();
  assert.deepEqual(calls, [["copy", "/repo/2"], ["close", 2], ["keep", 2]]);
  assert.equal(menu.others.off, false);
  assert.equal(repoTabActions(target, 1, actions).others.off, true);
});

test("opening deduplicates canonical paths, preserves the original on failure and cancellation, and raises other windows", async () => {
  const actions: string[] = [];
  const { tabs, state, from, errors } = windowTabs({
    async open(path) {
      if (path === "alias") return { kind: "existing", session: 1, repo: summary(1) };
      if (path === "elsewhere") return { kind: "otherWindow", window: "repo-2" };
      if (path === "untrusted") return { kind: "untrusted", repo: summary(3) };
      return opened(Number(path));
    },
    async history(id) { if (id === 3) throw new Error("folder disappeared"); return history; },
    async activate(id) { actions.push(`activate:${id}`); },
    async close(id) { actions.push(`close:${id}`); },
  });
  await tabs.open("1", from);
  await tabs.open("2", from);
  await tabs.open("untrusted", from);
  await tabs.open("elsewhere", from);
  await tabs.open("3", from);
  assert.equal(state.active, 2);
  assert.match(String(errors[0]), /folder disappeared/);
  await tabs.open("alias", from);
  assert.equal(state.active, 1);
  assert.deepEqual(state.tabs.map(tab => tab.id), [1, 2]);
  assert.deepEqual(actions, ["activate:1", "activate:2", "close:3", "activate:1"]);
});

test("an open that finishes while Welcome has a modal releases its session", async () => {
  const closed: number[] = [];
  const { tabs, state, from } = windowTabs({
    open: async () => opened(1),
    history: async () => history,
    activate: async () => { state.free = false; },
    close: async (id) => { closed.push(id); },
  });
  await tabs.open("1", from);
  assert.equal(state.active, null);
  assert.deepEqual(state.tabs, []);
  assert.deepEqual(closed, [1]);
});

test("a late session is closed when its source closes during the first History read", async () => {
  const reading = Promise.withResolvers<void>();
  const ready = Promise.withResolvers<History>();
  const activations: number[] = [];
  const closed: number[] = [];
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async (id) => {
      if (id === 2) { reading.resolve(); return ready.promise; }
      return history;
    },
    activate: async (id) => { activations.push(id); },
    close: async (id) => { closed.push(id); },
  });
  await tabs.open("1", from);
  const pending = tabs.open("2", { ...from, live: () => state.tabs.some(tab => tab.id === 1) });
  await reading.promise;
  await tabs.close(1);
  ready.resolve(history);
  await pending;
  assert.deepEqual(activations, [1]);
  assert.deepEqual(closed, [1, 2]);
  assert.deepEqual(state.tabs, []);
});

test("A → B → A leaves History, diff, fetch, Stop, and notices with their original session", async (t) => {
  const a = apiForSession(1);
  const b = apiForSession(2);
  const delayed = Promise.withResolvers<void>();
  const calls: [string, unknown][] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: { __TAURI_INTERNALS__: {
    transformCallback: () => 0,
    invoke(command: string, args: Record<string, unknown>) {
      calls.push([command, args.repoSession]);
      return command === "history" ? delayed.promise : Promise.resolve();
    },
  } } });
  t.after(() => { Reflect.deleteProperty(globalThis, "window"); });

  const { tabs, state, from, notices } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: (id) => apiForSession(id).activateRepoTab(),
    close: (id) => apiForSession(id).closeRepo(),
  });
  await tabs.open("1", from);
  const pending = a.history(2000).then(() => a.commitDiff("sha", null, false));
  await tabs.open("2", from);
  await b.fetchInBackground(null, 1, false);
  const notice = { session: 1, event: "repo-touched", payload: null } as const;
  tabs.receive(notice);
  await tabs.receive({ session: 1, event: "repo-activate-request", payload: null });
  await b.stopFetch(1);
  delayed.resolve();
  await pending;
  await api.openRepo("/legacy");
  assert.equal(state.active, 1);
  assert.deepEqual(notices, [notice]);
  assert.deepEqual(calls, [
    ["activate_repo_tab", 1], ["history", 1], ["activate_repo_tab", 2],
    ["fetch_in_background", 2], ["activate_repo_tab", 1], ["stop_fetch", 2],
    ["commit_diff", 1], ["open_repo", undefined],
  ]);
});

test("different worktrees keep separate commit drafts even after their panels return", () => {
  const source = fs.readFileSync(new URL("../src/lib/commitDrafts.svelte.ts", import.meta.url), "utf8").replace(/^export /gm, "");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022 });
  const compiled = compileModule(js, { filename: "commitDrafts.svelte.js", generate: "client" }).js.code
    .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
  const storage = new Map<string, string>();
  const { draftFor, saveDraft } = Function("$", "localStorage", `${compiled}; return { draftFor, saveDraft };`)(runtime, {
    getItem: (key: string) => storage.get(key),
    setItem: (key: string, value: string) => storage.set(key, value),
  }) as typeof Drafts;

  const a = draftFor("/common/main");
  a.summary = "Changes on main";
  const b = draftFor("/common/worktree");
  b.summary = "Changes on topic";
  saveDraft("/common/main", a);
  saveDraft("/common/worktree", b);
  assert.equal(draftFor("/common/main").summary, "Changes on main");
  assert.equal(draftFor("/common/worktree").summary, "Changes on topic");
  assert.deepEqual([...storage.values()].map(value => JSON.parse(value).summary), ["Changes on main", "Changes on topic"]);
});
