import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { stopped } from "../src/lib/api.ts";

const view = fs.readFileSync(new URL("../src/lib/PullFiles.svelte", import.meta.url), "utf8");
const shared = view.slice(view.indexOf("  let fetchId"), view.indexOf("</script>"));
const reads = view.slice(view.indexOf("  const pr = $derived"), view.indexOf("  /** The files with"));
const notes = view.slice(view.indexOf("  const notes: LineNotes"), view.indexOf("  /** New lines"));
const js = ts.transpile(`
  ${shared}
  function mount(lab, project, number) {
    let prState = $state.raw({ headSha: "head", base: "main", baseSha: lab ? null : "base", title: "Before", state: "open" });
    let draft = $state.raw(null);
    let refresh = $state(0);
    let settings = $state("initial");
    const diffSettings = () => settings;
    const session = {
      get pr() { return prState; }, lab, number, saves: { target: { project } }, loadViewing: async () => {},
      get draft() { return draft; }, shown: null,
    };
    ${reads}
    const me = "reviewer", commentable = () => true, lineNotes = () => {}, movedNotes = () => {};
    ${notes}
    return {
      update: (fields) => { prState = { ...prState, ...fields }; },
      draftOn: (commit) => { draft = { commit }; },
      choose: (since) => { choice = { since }; },
      refresh: () => { refresh++; },
      toggle: () => { whole = !whole; },
      settings: () => { settings = "changed"; },
      loadMore,
      sameLines,
      canComment: () => notes.can(null, null, 0),
      get since() { return since; },
      get comparison() { return comparison; },
      get diffs() { return diffs; },
      get loading() { return loading; },
      get error() { return error; },
      get stale() { return stale; },
    };
  }
`, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext });
const compiled = compileModule(js, { filename: "pullFiles.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function deferredCall(args: unknown[]) {
  return { args, ...Promise.withResolvers<any>() };
}
const settle = async () => {
  await new Promise<void>(resolve => setImmediate(resolve));
  runtime.flush();
};
const result = (base: string, head = "head") => ({ tips: { base, head }, stale: null });
const diff = (path: string) => ({ file: { path } });
const comparison = (from: string, targetId = "head") => ({ from, targetId, files: [diff(from).file] });

function fixture(lab = false) {
  const fetches: ReturnType<typeof deferredCall>[] = [];
  const compares: ReturnType<typeof deferredCall>[] = [];
  const pages: ReturnType<typeof deferredCall>[] = [];
  const record = (calls: ReturnType<typeof deferredCall>[], ...args: unknown[]) => {
    const call = deferredCall(args);
    calls.push(call);
    return call.promise;
  };
  const api = {
    pullFetch: (number: number, base: string, _target: unknown, _stale: boolean, id: number) =>
      record(fetches, "GitHub", number, base, id),
    gitlabFetch: (number: number, base: string, _target: unknown, _stale: boolean, id: number) =>
      record(fetches, "GitLab", number, base, id),
    compare: (...args: unknown[]) => record(compares, ...args),
    compareDiffPage: (...args: unknown[]) => record(pages, ...args),
    compareCounts: async () => [],
    stopReads: async () => {},
  };
  const mount = Function("$", "api", "untrack", "wholeByDefault", "stopped",
    `${compiled}; return mount;`)(runtime, api, runtime.untrack, () => false, stopped);
  function open(project = "acme/api", number = 1) {
    let state!: ReturnType<typeof mount>;
    const stop = runtime.effect_root(() => { state = mount(lab, project, number); });
    runtime.flush();
    return { state, stop };
  }
  return { ...open(), open, fetches, compares, pages };
}

test("Files fetches for revisions and explicit refresh, even after failure", async (t) => {
  const { state, fetches, compares, pages, stop } = fixture();
  t.after(stop);
  assert.deepEqual(fetches[0].args.slice(0, 3), ["GitHub", 1, "main"]);
  state.update({ title: "Metadata reload while fetching" });
  state.refresh();
  await settle();
  assert.equal(fetches.length, 1, "a refresh can share the fetch already in flight");
  fetches[0].resolve(result("first"));
  await settle();
  assert.deepEqual(compares[0].args, ["first", "head", "split"]);
  compares[0].resolve(comparison("first"));
  await settle();
  pages[0].resolve({ diffs: [diff("first")], next: null });
  await settle();
  state.update({ title: "Metadata reload after fetch", reviews: [] });
  state.settings();
  await settle();
  assert.deepEqual(pages[1].args.slice(0, 5), ["first", "head", 0, 4000, false]);
  state.toggle();
  await settle();
  assert.equal(fetches.length, 1);
  assert.deepEqual(pages[2].args.slice(0, 5), ["first", "head", 0, 4000, true]);

  state.refresh();
  await settle();
  assert.equal(fetches.length, 2);
  fetches[1].reject("offline");
  await settle();
  assert.equal(state.comparison.from, "first");
  assert.equal(state.stale, "offline");
  state.refresh();
  await settle();
  assert.equal(fetches.length, 3, "a completed failure can be retried at the same SHA");
  fetches[2].resolve(result("first"));
  await settle();
  assert.equal(state.stale, null);
  assert.equal(compares.length, 2, "identical comparisons in flight are shared");
  compares[1].resolve(comparison("first"));
  await settle();

  state.update({ baseSha: "base advanced" });
  await settle();
  state.update({ base: "other" });
  await settle();
  state.update({ headSha: "new head" });
  await settle();
  assert.deepEqual(fetches.slice(3).map(call => call.args[2]), ["main", "other", "other"]);
});

test("a stopped fetch clears loading without an error or losing the displayed files", async (t) => {
  const { state, fetches, compares, pages, stop } = fixture();
  t.after(stop);
  assert.equal(state.loading, true);
  fetches[0].reject("stopped");
  await settle();
  assert.equal(state.loading, false);
  assert.equal(state.error, null);
  assert.equal(state.stale, null);
  assert.equal(state.comparison, null);

  state.refresh();
  await settle();
  fetches[1].resolve(result("first"));
  await settle();
  compares[0].resolve(comparison("first"));
  await settle();
  pages[0].resolve({ diffs: [diff("first")], next: null });
  await settle();

  state.refresh();
  await settle();
  assert.equal(state.loading, true);
  fetches[2].reject("stopped");
  await settle();
  assert.equal(state.loading, false);
  assert.equal(state.error, null);
  assert.equal(state.stale, null);
  assert.deepEqual(state.comparison.files, [diff("first").file]);
  assert.deepEqual(state.diffs, [diff("first")]);
});

test("returning to Files shares its fetch, while another project or request gets its own", async (t) => {
  const { open, fetches, compares, state, stop } = fixture();
  stop();
  const reopened = open();
  assert.equal(fetches.length, 1);
  fetches[0].resolve(result("first"));
  await settle();
  assert.equal(compares.length, 1);
  compares[0].resolve(comparison("first"));
  await settle();
  assert.equal(state.comparison, null);
  assert.equal(reopened.state.comparison.from, "first");
  reopened.stop();
  open("acme/site").stop();
  t.after(open("acme/api", 2).stop);
  assert.equal(fetches.length, 3);
});

test("GitLab diff refs do not trigger a fetch for a target with the same name", async (t) => {
  const { state, fetches, stop } = fixture(true);
  t.after(stop);
  assert.deepEqual(fetches[0].args.slice(0, 3), ["GitLab", 1, "main"]);
  fetches[0].resolve(result("first"));
  await settle();
  state.update({ gitlab: { diff: { baseSha: "split", startSha: "old-tip", headSha: "head" } } });
  await settle();
  assert.equal(fetches.length, 1);
  state.refresh();
  await settle();
  assert.equal(fetches.length, 2);
});

test("refresh retries a failed first diff page at the same tips", async (t) => {
  const { state, fetches, compares, pages, stop } = fixture();
  t.after(stop);
  const load = async (i: number) => {
    fetches[i].resolve(result("first"));
    await settle();
    compares[i].resolve(comparison("first"));
    await settle();
  };
  await load(0);
  pages[0].reject("page failed");
  await settle();
  assert.equal(state.error, "page failed");
  state.refresh();
  await settle();
  await load(1);
  pages[1].resolve({ diffs: [diff("first")], next: null });
  await settle();
  assert.equal(state.error, null);
  assert.equal(state.diffs[0].file.path, "first");
});

test("a stale fetch, comparison and page cannot replace Files for the latest base", async (t) => {
  const { state, fetches, compares, pages, stop } = fixture();
  t.after(stop);
  fetches[0].resolve(result("first"));
  await settle();
  compares[0].resolve(comparison("first"));
  await settle();
  pages[0].resolve({ diffs: [diff("first")], next: 7 });
  await settle();
  const more = state.loadMore();
  assert.equal(more, state.loadMore());

  for (const base of ["second", "third", "latest"]) {
    state.update({ base });
    await settle();
    if (base === "second") {
      fetches[1].resolve(result(base));
      await settle();
    }
  }
  fetches[3].resolve(result("latest"));
  await settle();
  compares[2].resolve(comparison("latest"));
  await settle();
  pages[2].resolve({ diffs: [diff("latest")], next: null });
  await settle();
  fetches[2].reject("stale fetch");
  compares[1].resolve(comparison("second"));
  pages[1].reject("stale page");
  await more;
  await settle();
  assert.equal(state.comparison.from, "latest");
  assert.deepEqual(state.diffs.map((d: ReturnType<typeof diff>) => d.file.path), ["latest"]);
  assert.equal(state.error, null);
  assert.equal(state.stale, null);
});

test("A to B to A starts a fresh fetch for A and a late completion cannot evict it", async (t) => {
  const { state, fetches, compares, stop } = fixture();
  t.after(stop);
  state.update({ headSha: "B" });
  await settle();
  state.update({ headSha: "head" });
  await settle();
  const ids = fetches.map(call => Number(call.args[3]));
  assert.equal(ids.length, 3);
  assert.ok(ids[0] < ids[1] && ids[1] < ids[2]);
  fetches[0].resolve(result("old A"));
  fetches[1].reject("stopped");
  await settle();
  assert.equal(compares.length, 0);
  assert.equal(state.loading, true);
  state.refresh();
  await settle();
  assert.equal(fetches.length, 3);
  fetches[2].resolve(result("fresh A"));
  await settle();
  assert.deepEqual(compares[0].args, ["fresh A", "head", "split"]);
  compares[0].resolve(comparison("fresh A"));
  await settle();
  assert.equal(state.comparison.from, "fresh A");
  assert.equal(state.error, null);
});

test("a stale snapshot keeps the draft on its lines even when the forge reports a new head", async (t) => {
  const { state, fetches, compares, stop } = fixture();
  t.after(stop);
  state.draftOn("head");
  fetches[0].resolve(result("root"));
  await settle();
  compares[0].resolve(comparison("root"));
  await settle();
  state.update({ headSha: "new head" });
  await settle();
  fetches[1].reject("offline");
  await settle();
  compares[1].resolve(comparison("root"));
  await settle();
  assert.equal(state.stale, "offline");
  assert.equal(state.sameLines("root"), true);
  assert.equal(compares.length, 2, "the draft and the displayed head are already the same");
});

test("after Stop, switching modes on the displayed snapshot waits for the new diff page", async (t) => {
  const { state, fetches, compares, pages, stop } = fixture();
  t.after(stop);
  state.draftOn("review");
  fetches[0].resolve(result("root"));
  await settle();
  compares[0].resolve(comparison("root"));
  await settle();
  pages[0].resolve({ diffs: [diff("root")], next: null });
  await settle();
  state.update({ headSha: "new head" });
  await settle();
  fetches[1].reject("stopped");
  await settle();
  state.choose(true);
  await settle();
  assert.equal(compares.length, 2, "the immutable comparison already in flight is shared");
  assert.deepEqual(compares[1].args, ["review", "head", "tips"]);
  assert.equal(state.since, false);
  assert.equal(state.canComment(), true);
  compares[1].resolve(comparison("review"));
  await settle();
  assert.equal(state.since, false);
  assert.equal(state.diffs[0].file.path, "root");
  pages[1].resolve({ diffs: [diff("review")], next: null });
  await settle();
  assert.equal(state.since, true);
  assert.equal(state.canComment(), false);
  assert.equal(state.diffs[0].file.path, "review");

  state.choose(false);
  await settle();
  compares[2].resolve(comparison("root"));
  await settle();
  assert.equal(state.since, true);
  assert.equal(state.canComment(), false);
  pages[2].resolve({ diffs: [diff("root")], next: null });
  await settle();
  assert.equal(state.since, false);
  assert.equal(state.canComment(), true);
  assert.equal(state.diffs[0].file.path, "root");
  assert.equal(state.error, null);
  assert.equal(state.stale, null);
});

test("late mode pages cannot change the selected mode or comments for a newer snapshot", async (t) => {
  const { state, fetches, compares, pages, stop } = fixture();
  t.after(stop);
  state.draftOn("review");
  fetches[0].resolve(result("root"));
  await settle();
  compares[0].resolve(comparison("root"));
  await settle();
  pages[0].resolve({ diffs: [diff("root")], next: null });
  state.choose(true);
  await settle();
  compares[1].resolve(comparison("review"));
  await settle();
  assert.equal(pages.length, 2);

  state.choose(false);
  state.update({ headSha: "new head" });
  await settle();
  fetches[1].resolve(result("new root", "new head"));
  await settle();
  compares[2].resolve(comparison("new root", "new head"));
  await settle();
  pages[2].resolve({ diffs: [diff("new root")], next: null });
  pages[1].resolve({ diffs: [diff("review")], next: null });
  await settle();
  assert.equal(state.since, false);
  assert.equal(state.canComment(), true);
  assert.equal(state.diffs[0].file.path, "new root");
  assert.equal(state.error, null);
});
