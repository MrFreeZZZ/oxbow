import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import type * as Drafts from "../src/lib/commitDrafts.svelte.ts";
import { test } from "node:test";
import { api, apiForSession, type RepoNotice, type RepoTabOpen } from "../src/lib/api.ts";
import { DIALOG_IN_THE_WAY, isPending, readTabLayout, repoTabActions, RepoTabs, saveTabLayout, type OpenFrom, type OpenTab, type ShownTab } from "../src/lib/repoTabs.ts";
import type { History, RepoSummary } from "../src/lib/types.ts";

const summary = (id: number): RepoSummary => ({ path: `/repo/${id}`, canonicalPath: `/repo/${id}`, name: String(id), restricted: [], untrusted: null });
const history = {} as History;
const opened = (session: number): RepoTabOpen => ({ kind: "opened", session, repo: summary(session) });

type TabBackend = ConstructorParameters<typeof RepoTabs>[0];

function windowTabs(
  backend: Omit<TabBackend, "closeBatch"> & Partial<Pick<TabBackend, "closeBatch">>,
  beforeClose = async (_id: number) => true,
  hooks: { canSwitch?: () => boolean; switchProblem?: () => string | null } = {},
) {
  const state = { tabs: [] as readonly ShownTab[], active: null as number | null, highlighted: null as number | null, free: true };
  const errors: unknown[] = [];
  const notices: RepoNotice[] = [];
  let tabs!: RepoTabs;
  const from: OpenFrom = {
    live: () => true,
    trust: async () => null,
    reopened: () => {},
    fail: async (_path, error) => { errors.push(error); },
  };
  tabs = new RepoTabs({
    closeBatch: async (ids, next, previous) => {
      if (next !== null && next !== previous) await backend.activate(next);
      for (const id of ids) await backend.close(id);
    },
    ...backend,
  }, {
    show: (tabs, active, highlighted) => Object.assign(state, { tabs, active, highlighted }),
    canSwitch: hooks.canSwitch ?? (() => state.free),
    switchProblem: hooks.switchProblem,
    beforeClose,
    whileClosing: (run) => run(),
    notice: (notice) => notices.push(notice),
    loadTab: (path) => tabs.open(path, from),
  });
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

test("dragging a tab reorders the list and leaves the active repository in place", async () => {
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: async () => {},
    close: async () => {},
  });
  for (const id of ["1", "2", "3"]) await tabs.open(id, from);
  await tabs.move(1, 2);
  assert.deepEqual(state.tabs.map(tab => tab.id), [2, 3, 1]);
  assert.equal(state.active, 3);
  await tabs.move(3, 0);
  assert.deepEqual(state.tabs.map(tab => tab.id), [3, 2, 1]);
  assert.equal(state.active, 3);
  await tabs.move(3, 0);
  await tabs.move(9, 1);
  assert.deepEqual(state.tabs.map(tab => tab.id), [3, 2, 1]);
});

test("the next run opens only the active repository and keeps the others until they are shown", async () => {
  let saved: string | null = null;
  const storage = {
    getItem: () => saved,
    setItem: (_key: string, value: string) => { saved = value; },
  };
  const original = [1, 2, 3, 4].map(id => ({ id, repo: summary(id), history }));
  original[0].repo.canonicalPath = "/canonical/worktree";
  assert.equal(readTabLayout(storage), null);
  saveTabLayout(original, 2, storage);
  const layout = readTabLayout(storage);
  assert.ok(layout);
  const opened: string[] = [];
  const { tabs, state, from, errors } = windowTabs({
    async open(path) {
      opened.push(path);
      const original = Number(path.at(-1));
      if (original === 3) throw new Error("Folder is unavailable");
      return { kind: "opened", session: original + 10, repo: { ...summary(original), canonicalPath: original === 1 ? "/canonical/worktree" : path } };
    },
    history: async () => history,
    activate: async () => {},
    close: async () => {},
  });
  await tabs.restore(layout, path => tabs.open(path, from));
  assert.deepEqual(opened, ["/repo/2"]);
  assert.deepEqual(state.tabs.map(tab => tab.repo.path), ["/repo/1", "/repo/2", "/repo/3", "/repo/4"]);
  assert.equal(isPending(state.tabs[0]), true);
  assert.equal(state.tabs[0].repo.canonicalPath, "/canonical/worktree");
  assert.equal(isPending(state.tabs[1]), false);
  assert.equal(state.active, 12);
  assert.equal(errors.length, 0);

  await tabs.activate(state.tabs[0].id);
  assert.equal(state.active, 11);
  assert.deepEqual(state.tabs.map(tab => tab.id), [11, 12, state.tabs[2].id, state.tabs[3].id]);
  assert.equal(isPending(state.tabs[0]), false);

  const unavailable = state.tabs[2].id;
  await tabs.activate(unavailable);
  assert.equal(state.active, 11);
  assert.equal(state.highlighted, 11);
  assert.equal(isPending(state.tabs[2]), true);
  assert.match(String(errors[0]), /Folder is unavailable/);

  await tabs.close(state.tabs[3].id);
  assert.deepEqual(state.tabs.map(tab => tab.repo.path), ["/repo/1", "/repo/2", "/repo/3"]);
  assert.equal(state.active, 11);

  saveTabLayout([], null, storage);
  assert.deepEqual(readTabLayout(storage), { paths: [], active: null });
  saved = '{"paths":[42],"active":null}';
  assert.equal(readTabLayout(storage), null);
});

test("closing the active tab opens the next restored repository before closing", async () => {
  const closed: number[] = [];
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path.at(-1))),
    history: async () => history,
    activate: async () => {},
    close: async (id) => { closed.push(id); },
  });
  await tabs.restore({ paths: ["/repo/1", "/repo/2", "/repo/4"], active: "/repo/1" }, path => tabs.open(path, from));
  await tabs.close(1);
  assert.equal(state.active, 2);
  assert.deepEqual(state.tabs.map(tab => tab.repo.path), ["/repo/2", "/repo/4"]);
  assert.deepEqual(closed, [1]);
});

test("an open tab stays selected when a restored tab finishes opening afterwards", async () => {
  const reading = Promise.withResolvers<void>();
  let release: (value: History) => void = () => {};
  const activated: number[] = [];
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path.at(-1))),
    history: async (id) => {
      if (id === 1) {
        reading.resolve();
        return new Promise<History>((resolve) => { release = resolve; });
      }
      return history;
    },
    activate: async (id) => { activated.push(id); },
    close: async () => {},
  });
  await tabs.restore({ paths: ["/repo/1", "/repo/2"], active: "/repo/2" }, path => tabs.open(path, from));
  const opening = tabs.activate(state.tabs[0].id);
  await reading.promise;
  await tabs.activate(2);
  release(history);
  await opening;
  assert.equal(state.active, 2);
  assert.equal(state.highlighted, 2);
  assert.equal(isPending(state.tabs[0]), false);
  assert.equal(activated.at(-1), 2);
});

test("turning tabs off drops repositories that have not been opened", async () => {
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path.at(-1))),
    history: async () => history,
    activate: async () => {},
    close: async () => {},
  });
  await tabs.restore({ paths: ["/repo/1", "/repo/2"], active: "/repo/1" }, path => tabs.open(path, from));
  tabs.setMode(false);
  assert.deepEqual(state.tabs.map(tab => tab.repo.path), ["/repo/1"]);
  assert.equal(state.active, 1);
});

test("closing the other tabs checks all guards before selecting the target and closing anything", async () => {
  const events: string[] = [];
  let blocked = 3;
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: async () => {},
    close: async () => assert.fail("closed outside the batch"),
    closeBatch: async (ids, active) => { events.push(`close:${ids.join(",")};keep:${active}`); },
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
  assert.deepEqual(events, ["guard:1", "guard:3", "close:1,3;keep:2"]);
  assert.deepEqual(state.tabs.map(tab => tab.id), [2]);
  assert.equal(state.active, 2);
});

test("the tab context menu targets the tab that opened it", () => {
  const calls: unknown[] = [];
  const target = { id: -1, repo: { ...summary(2), canonicalPath: "/canonical/worktree" }, pending: true } as const;
  const actions = {
    copyPath: (path: string) => calls.push(["copy", path]),
    close: (id: number) => calls.push(["close", id]),
    closeOthers: (id: number) => calls.push(["keep", id]),
    saveTag: (path: string, id: string | null) => calls.push(["tag", path, id]),
  };
  const menu = repoTabActions(target, 3, actions);
  menu.copy.run();
  menu.tag("gray");
  menu.tag("gray");
  menu.tag(null);
  menu.close.run();
  menu.others.run();
  assert.deepEqual(calls, [
    ["copy", "/repo/2"], ["tag", "/canonical/worktree", "gray"], ["tag", "/canonical/worktree", "gray"],
    ["tag", "/canonical/worktree", null], ["close", -1], ["keep", -1],
  ]);
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

test("replacing a repository while a dialog is open reports it and releases the new session", async () => {
  const closed: number[] = [];
  let dialog = false;
  const { tabs, state, from, errors } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async (id) => {
      if (id === 2) dialog = true;
      return history;
    },
    activate: async (id) => { if (id === 2) assert.fail("dialog open activated"); },
    close: async (id) => { closed.push(id); },
    closeBatch: async () => { throw new Error("replaced despite the dialog"); },
  }, async () => true, {
    canSwitch: () => !dialog,
    switchProblem: () => dialog ? DIALOG_IN_THE_WAY : null,
  });
  tabs.setMode(false);
  await tabs.open("1", from);
  await tabs.open("2", from);
  assert.equal(state.active, 1);
  assert.deepEqual(state.tabs.map(tab => tab.id), [1]);
  assert.deepEqual(closed, [2]);
  assert.match(String(errors[0]), new RegExp(DIALOG_IN_THE_WAY));
});

test("an open that becomes blocked during its first History read releases its provisional session", async () => {
  const closed: number[] = [];
  const { tabs, state, from } = windowTabs({
    open: async () => opened(1),
    history: async () => { state.free = false; return history; },
    activate: async () => assert.fail("blocked open activated"),
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
  let full = false;
  const { draftFor, saveDraft, keepCommitMessage } = Function("$", "localStorage", `${compiled}; return { draftFor, saveDraft, keepCommitMessage };`)(runtime, {
    getItem: (key: string) => storage.get(key),
    setItem: (key: string, value: string) => {
      if (full) throw new Error("Full");
      storage.set(key, value);
    },
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
  full = true;
  assert.match(keepCommitMessage("/common/main")!, /could not save a commit message draft/);
  assert.equal(keepCommitMessage("/common/unvisited"), null);
  full = false;
  assert.equal(keepCommitMessage("/common/main"), null);
});

test("legacy replacement preserves the previous screen on refusal and enabling tabs keeps its session", async () => {
  let refused = false;
  const { tabs, state, from, errors } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: async () => {},
    close: async () => {},
    closeBatch: async (ids, active, previous) => {
      assert.deepEqual(ids, [previous]);
      if (refused) throw new Error("Git command in progress");
      assert.equal(active, 2);
    },
  });
  tabs.setMode(false);
  await tabs.open("1", from);
  const first = state.tabs[0];
  refused = true;
  await tabs.open("2", from);
  assert.deepEqual(state.tabs, [first]);
  assert.match(String(errors[0]), /Git command in progress/);

  refused = false;
  await tabs.open("2", from);
  const second = state.tabs[0];
  assert.equal(state.active, 2);
  tabs.setMode(true);
  await tabs.open("3", from);
  assert.equal(state.tabs[0], second);
  assert.deepEqual(state.tabs.map(tab => tab.id), [2, 3]);
  tabs.setMode(false, [2]);
  assert.equal(state.active, 3);
  assert.deepEqual(state.tabs.map(tab => tab.id), [3]);
});

test("a close batch rejected by the backend leaves every tab and the active selection in place", async () => {
  const { tabs, state, from } = windowTabs({
    open: async (path) => opened(Number(path)),
    history: async () => history,
    activate: async () => {},
    close: async () => assert.fail("closed outside the batch"),
    closeBatch: async () => { throw new Error("Waiting for a Git operation"); },
  });
  for (const id of ["1", "2", "3"]) await tabs.open(id, from);
  await assert.rejects(tabs.closeOthers(2), /Waiting for a Git operation/);
  assert.deepEqual(state.tabs.map(tab => tab.id), [1, 2, 3]);
  assert.equal(state.active, 3);
});
