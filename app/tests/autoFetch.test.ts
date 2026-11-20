import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import ts from "typescript";

const app = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");

type FetchReply = { retryAfterMs: number | null } | { updated: number; gaveWay: boolean; remote: string | null };

function setup(fetch: (auto: boolean) => Promise<FetchReply>, timeOrigin = performance.timeOrigin) {
  const activitySource = fs.readFileSync(new URL("../src/lib/activity.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const fetchSource = app.slice(app.indexOf("  async function backgroundFetch("), app.indexOf("  /** Fix… of a failed fetch"));
  const js = ts.transpile(activitySource + fetchSource, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const calls: string[] = [];
  const requests: { id: number; remote: string | null }[] = [];
  let onEvent: (event: unknown) => void = () => {};
  const globals = {
    $state: (value: unknown) => value,
    performance: { timeOrigin },
    getCurrentWindow: () => ({ listen: async (_name: string, callback: typeof onEvent) => { onEvent = callback; } }),
    waitingWords: () => "Waiting…",
    repo: { path: "repo" },
    fetchTarget: () => ({ remote: "origin", label: "origin" }),
    api: {
      fetchInBackground: (remote: string | null, id: number, auto: boolean) => {
        calls.push(auto ? "auto" : "manual");
        requests.push({ id, remote });
        return fetch(auto);
      },
      stopFetch: async (id: number) => calls.push(`stop:${id}`),
      refreshRemoteTags: async () => calls.push("ls-remote"),
    },
    refreshQuietly: async () => calls.push("refresh"),
    confirm: { say: () => calls.push("notice") },
  };
  const { activity, backgroundFetch } = Function(
    ...Object.keys(globals), `${js}; return {activity, backgroundFetch};`,
  )(...Object.values(globals));
  const event = (fetch: number, remote: string | null, event: unknown) => onEvent({ payload: { fetch, remote, event } });
  return { activity, backgroundFetch, calls, requests, event };
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
  const { activity, backgroundFetch, calls, requests, event } = setup((timer) => timer ? auto.promise : manual.promise);
  const tick = backgroundFetch(true);
  assert.equal(requests[0].remote, null, "the timer does not use the window's remote");
  assert.equal(activity.running, null);
  const user = backgroundFetch(false);
  assert.equal(requests[1].remote, "origin");
  assert.equal(activity.running?.title, "Fetching origin");

  event(requests[0].id, null, { kind: "command", display: "git fetch --all" });
  auto.resolve({ retryAfterMs: 5000 });
  await tick;
  assert.equal(activity.running?.id, requests[1].id);
  assert.deepEqual(activity.lines, []);
  assert.deepEqual(activity.recent, []);

  manual.resolve({ updated: 1, gaveWay: false, remote: "origin" });
  await user;
  assert.equal(activity.recent[0]?.title, "Fetched origin");
  assert.deepEqual(calls, ["auto", "manual", "ls-remote", "refresh", "notice"]);
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
    activity: { running: null },
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
});
