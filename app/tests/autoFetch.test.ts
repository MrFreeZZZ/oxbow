import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import ts from "typescript";
import { Reloads, Tickets } from "../src/lib/loads.ts";

const app = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");

type FetchReply = { retryAfterMs: number | null } | { updated: number; gaveWay: boolean; remote: string | null };
type SyncRequest = { kind: "pull" | "push"; remote: string; mode?: string };

function setup(fetch: (auto: boolean) => Promise<FetchReply>, timeOrigin = performance.timeOrigin, loader?: {
  refreshForSync(): Promise<void>;
  refreshQuietly(): Promise<void>;
}) {
  const activitySource = fs.readFileSync(new URL("../src/lib/activity.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const syncSource = app.slice(app.indexOf("  async function sync("), app.indexOf("  /** What Fetch fetches"));
  const fetchSource = app.slice(app.indexOf("  async function backgroundFetch("), app.indexOf("  /** Fix… of a failed fetch"));
  const js = ts.transpile(activitySource + syncSource + fetchSource, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const calls: string[] = [];
  const requests: { id: number; remote: string | null }[] = [];
  const refresh = Promise.withResolvers<void>();
  const confirmed = Promise.withResolvers<SyncRequest>();
  const syncRefresh = () => {
    calls.push("sync-refresh");
    return refresh.promise;
  };
  let onEvent: (event: unknown) => void = () => {};
  const globals = {
    $state: (value: unknown) => value,
    performance: { timeOrigin },
    getCurrentWindow: () => ({ listen: async (_name: string, callback: typeof onEvent) => { onEvent = callback; } }),
    waitingWords: () => "Waiting…",
    repo: { path: "repo" },
    history: { head: { branch: "feature" }, submodules: [] },
    remoteCtx: { remote: "origin" },
    fetchTarget: () => ({ remote: "origin", label: "origin" }),
    api: {
      fetchInBackground: (remote: string | null, id: number, auto: boolean) => {
        calls.push(auto ? "auto" : "manual");
        requests.push({ id, remote });
        return fetch(auto);
      },
      stopFetch: async (id: number) => calls.push(`stop:${id}`),
      refreshRemoteTags: async () => calls.push("ls-remote"),
      pullSetup: async () => ({ mode: "rebase", autostash: true }),
    },
    refreshQuietly: async () => {
      calls.push("refresh");
      await loader?.refreshQuietly();
    },
    refresh: syncRefresh,
    loader: loader ?? { refreshForSync: syncRefresh },
    syncBlock: () => null,
    prefs: { get: () => false },
    pullRequest: ({ remote }: { remote: string }, { mode }: { mode: string }) => ({ kind: "pull", remote, mode }),
    pushRequest: ({ remote }: { remote: string }) => ({ kind: "push", remote }),
    confirm: {
      say: () => calls.push("notice"),
      run: async (request: SyncRequest) => {
        calls.push(`confirm:${request.kind}`);
        confirmed.resolve(request);
      },
    },
    checkRemoteTags: () => {},
    openConflicts: () => {},
  };
  const { activity, backgroundFetch, sync, setRemote, switchRepo } = Function(
    ...Object.keys(globals), `${js}; return {
      activity, backgroundFetch, sync,
      setRemote: (remote) => { remoteCtx = { remote }; },
      switchRepo: () => { repo = { path: "other" }; return activity.reset(); },
    };`,
  )(...Object.values(globals));
  const event = (fetch: number, remote: string | null, event: unknown) => onEvent({ payload: { fetch, remote, event } });
  return { activity, backgroundFetch, sync, calls, requests, event, refresh, confirmed, setRemote, switchRepo };
}

test("a skipped timer tick leaves Activity and remote tags alone", async () => {
  let reply: FetchReply = { retryAfterMs: null };
  const { activity, backgroundFetch, calls } = setup(async () => reply);
  assert.equal(await backgroundFetch(true), null);
  reply = { retryAfterMs: 5000 };
  assert.equal(await backgroundFetch(true), 5000);
  reply = { updated: 0, gaveWay: true, remote: null };
  await backgroundFetch(true);
  assert.deepEqual(calls, ["auto", "auto", "auto"]);
  assert.deepEqual(activity.recent, []);
  assert.equal(activity.running, null);

  reply = { updated: 1, gaveWay: false, remote: "origin" };
  await backgroundFetch(false);
  assert.equal(activity.recent[0]?.state, "done");
  assert.deepEqual(calls.slice(3), ["manual", "ls-remote", "refresh", "notice"]);
});

test("manual Fetch can start during an invisible timer tick and survives its late reply", async () => {
  const auto = Promise.withResolvers<FetchReply>();
  const manual = Promise.withResolvers<FetchReply>();
  const { activity, backgroundFetch, sync, calls, requests, event } = setup((timer) => timer ? auto.promise : manual.promise);
  const tick = backgroundFetch(true);
  assert.equal(requests[0].remote, null, "the timer does not use the window's remote");
  assert.equal(activity.running, null);
  await sync("push");
  const [waiting] = activity.queued;
  const user = backgroundFetch(false);
  assert.equal(requests[1].remote, "origin");
  assert.equal(activity.running?.title, "Fetching origin");

  event(requests[0].id, null, { kind: "command", display: "git fetch --all" });
  auto.resolve({ retryAfterMs: 5000 });
  await tick;
  assert.equal(activity.running?.id, requests[1].id);
  assert.deepEqual(activity.lines, []);
  assert.deepEqual(activity.recent, []);
  assert.deepEqual(activity.queued, [waiting]);

  activity.unqueue(waiting.id);
  manual.resolve({ updated: 1, gaveWay: false, remote: "origin" });
  await user;
  assert.equal(activity.recent[0]?.title, "Fetched origin");
  assert.deepEqual(calls, ["auto", "manual", "ls-remote", "refresh", "notice"]);
});

for (const [kind, title] of [["pull", "Pull into feature"], ["push", "Push feature"]] as const) {
  test(`${kind} queues behind auto-fetch before its first event and confirms once after refreshing`, async () => {
    const auto = Promise.withResolvers<FetchReply>();
    const { activity, backgroundFetch, sync, calls, event, requests, refresh, confirmed, setRemote } = setup(() => auto.promise);
    const tick = backgroundFetch(true);
    const mode = kind === "pull" ? "merge" : undefined;
    await sync(kind, mode);
    assert.equal(activity.running, null);
    assert.equal(activity.open, true);
    assert.equal(activity.queued.length, 1);
    assert.equal(activity.queued[0].title, title);
    await backgroundFetch(true);
    assert.deepEqual(calls, ["auto"]);

    event(requests[0].id, "upstream", { kind: "waiting" });
    assert.equal(activity.running?.title, "Fetching upstream");
    assert.deepEqual(calls, ["auto"]);
    auto.resolve({ updated: 1, gaveWay: false, remote: "upstream" });
    await tick;
    assert.deepEqual(activity.queued, []);
    assert.deepEqual(calls, ["auto", "sync-refresh", "ls-remote", "refresh"]);

    setRemote("upstream");
    refresh.resolve();
    assert.deepEqual(await confirmed.promise, { kind, remote: "upstream", ...(mode && { mode }) });
    assert.equal(calls.filter((call) => call.startsWith("confirm:")).length, 1);
  });
}

for (const retryAfterMs of [null, 5000]) {
  test(`auto-fetch releases a queued Push on a retry of ${retryAfterMs} without events`, async () => {
    const auto = Promise.withResolvers<FetchReply>();
    const { activity, backgroundFetch, sync, calls, refresh, confirmed } = setup(() => auto.promise);
    const tick = backgroundFetch(true);
    await sync("push");
    assert.equal(activity.running, null);
    assert.equal(activity.queued.length, 1);
    assert.deepEqual(calls, ["auto"]);

    auto.resolve({ retryAfterMs });
    assert.equal(await tick, retryAfterMs);
    assert.equal(activity.running, null);
    assert.deepEqual(activity.queued, []);
    assert.deepEqual(activity.recent, []);
    assert.deepEqual(calls, ["auto", "sync-refresh"]);
    refresh.resolve();
    assert.deepEqual(await confirmed.promise, { kind: "push", remote: "origin" });
  });
}

test("switching repositories drops a queued Pull and ignores the old auto-fetch's result", async () => {
  const auto = Promise.withResolvers<FetchReply>();
  const { activity, backgroundFetch, sync, calls, requests, switchRepo } = setup(() => auto.promise);
  const tick = backgroundFetch(true);
  await sync("pull");
  assert.equal(activity.queued.length, 1);
  await switchRepo();
  assert.deepEqual(activity.queued, []);
  auto.resolve({ updated: 1, gaveWay: false, remote: "origin" });
  await tick;
  assert.deepEqual(calls, ["auto", `stop:${requests[0].id}`]);
  assert.deepEqual(activity.recent, []);
});

test("switching repositories during the resumed Push's refresh prevents confirmation", async () => {
  const auto = Promise.withResolvers<FetchReply>();
  const { backgroundFetch, sync, calls, refresh, setRemote, switchRepo } = setup(() => auto.promise);
  const tick = backgroundFetch(true);
  await sync("push");
  auto.resolve({ retryAfterMs: null });
  await tick;
  assert.deepEqual(calls, ["auto", "sync-refresh"]);

  await switchRepo();
  setRemote("other");
  refresh.resolve();
  await refresh.promise;
  assert.deepEqual(calls, ["auto", "sync-refresh"]);
});

test("queued Pull waits for the older history page and the reload that reads fresh refs", async () => {
  const source = fs.readFileSync(new URL("../src/lib/historyLoad.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const initial = { remote: "origin", loaded: 2000, truncated: true, rows: [], worktrees: [], submodules: [] };
  const page = Promise.withResolvers<typeof initial>();
  const reloaded = Promise.withResolvers<typeof initial>();
  const reads: number[] = [];
  const globals = {
    Reloads,
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    api: {
      history: (limit: number) => {
        reads.push(limit);
        return reads.length === 1 ? page.promise : reloaded.promise;
      },
    },
    withRows: (history: unknown) => history,
    carryStates: (history: unknown) => history,
    settle: (history: unknown) => ({ history, states: {} }),
    oplog: { load: () => {} },
    lfs: { load: () => {} },
  };
  const HistoryLoader = Function(...Object.keys(globals), `${js}; return HistoryLoader;`)(...Object.values(globals));
  const loader = new HistoryLoader({ repo: {}, tickets: new Tickets() }, (next: typeof initial) => setRemote(next.remote));
  loader.history = initial;
  const paging = loader.loadOlder();
  assert.deepEqual(reads, [4000]);
  assert.equal(await loader.loadOlder(), false);

  const auto = Promise.withResolvers<FetchReply>();
  const { backgroundFetch, sync, calls, confirmed, setRemote } = setup(() => auto.promise, performance.timeOrigin, loader);
  const tick = backgroundFetch(true);
  await sync("pull", "merge");
  auto.resolve({ updated: 1, gaveWay: false, remote: "upstream" });
  await tick;
  assert.deepEqual(calls, ["auto", "ls-remote", "refresh"]);
  assert.deepEqual(reads, [4000]);

  page.resolve({ ...initial, loaded: 4000 });
  await paging;
  assert.deepEqual(reads, [4000, 4000]);
  assert.deepEqual(calls, ["auto", "ls-remote", "refresh"]);

  reloaded.resolve({ ...initial, loaded: 4000, remote: "upstream" });
  assert.deepEqual(await confirmed.promise, { kind: "pull", remote: "upstream", mode: "merge" });
  assert.deepEqual(reads, [4000, 4000]);
  assert.deepEqual(calls, ["auto", "ls-remote", "refresh", "confirm:pull"]);
});

test("Activity shows an accepted tick and scopes Stop, reset and late events to its request", async () => {
  const auto = Promise.withResolvers<FetchReply>();
  const manual = Promise.withResolvers<FetchReply>();
  const { activity, backgroundFetch, calls, requests, event } = setup((timer) => timer ? auto.promise : manual.promise);
  const tick = backgroundFetch(true);
  const [{ id }] = requests;
  event(id, "upstream", { kind: "waiting" });
  assert.equal(activity.running?.title, "Fetching upstream");
  assert.equal(activity.running?.sub, "Waiting…");
  activity.stop();
  await activity.reset();
  assert.deepEqual(calls, ["auto", `stop:${id}`, `stop:${id}`]);
  const user = backgroundFetch(false);
  event(id, "upstream", { kind: "command", display: "git fetch upstream" });
  auto.reject({ kind: "cancelled" });
  await tick;
  assert.equal(activity.running?.id, requests[1].id);
  assert.deepEqual(activity.lines, []);
  assert.deepEqual(activity.recent, []);
  manual.resolve({ updated: 0, gaveWay: false, remote: "origin" });
  await user;
  assert.equal(activity.recent[0]?.state, "done");
});

test("a reloaded WebView starts above its previous fetch numbers", async () => {
  const fetch = async (): Promise<FetchReply> => ({ updated: 0, gaveWay: false, remote: "origin" });
  const previous = setup(fetch, 1_800_000_000_000);
  await previous.backgroundFetch(true);
  await previous.backgroundFetch(false);
  const reloaded = setup(fetch, 1_800_000_000_001);
  await reloaded.backgroundFetch(false);
  assert.ok(reloaded.requests[0].id > previous.requests[1].id);
  assert.ok(Number.isSafeInteger(reloaded.requests[0].id));
});

test("auto-fetch replaces the next interval for a timed retry and keeps it for a null retry", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const source = app.slice(app.indexOf("  const hasRemotes ="), app.indexOf("  async function chooseRepo("));
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const calls: boolean[] = [];
  const first = Promise.withResolvers<number>();
  const globals = {
    history: { remotes: ["origin"] },
    $derived: (value: boolean) => value,
    $effect: (start: () => () => void) => t.after(start()),
    overviewShown: false,
    repo: {},
    prefs: { get: (key: string) => ({ "oxbow.fetch.auto": true, "oxbow.fetch.interval": 1 })[key] },
    confirm: { request: null },
    document: { hidden: false },
    loading: false,
    activity: { hasFetch: false },
    backgroundFetch: (auto: boolean) => {
      calls.push(auto);
      return calls.length === 1 ? first.promise : Promise.resolve(null);
    },
  };
  Function(...Object.keys(globals), js)(...Object.values(globals));
  t.mock.timers.tick(5000);
  assert.deepEqual(calls, [true]);
  first.resolve(10_000);
  await first.promise;

  t.mock.timers.tick(9999);
  assert.equal(calls.length, 1);
  t.mock.timers.tick(1);
  assert.deepEqual(calls, [true, true]);
  await Promise.resolve();

  t.mock.timers.tick(59_999);
  assert.equal(calls.length, 2, "the original timer was cleared");
  t.mock.timers.tick(1);
  assert.deepEqual(calls, [true, true, true], "a null retry keeps the regular interval");
  await Promise.resolve();

  globals.activity.hasFetch = true;
  t.mock.timers.tick(60_000);
  assert.equal(calls.length, 3, "a pending fetch skips the next tick");
  globals.activity.hasFetch = false;
  t.mock.timers.tick(60_000);
  assert.equal(calls.length, 4);
});
