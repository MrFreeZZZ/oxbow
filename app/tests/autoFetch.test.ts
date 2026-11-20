import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import ts from "typescript";

const app = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");

test("a skipped timer tick leaves Activity and remote tags alone", async () => {
  const activitySource = fs.readFileSync(new URL("../src/lib/activity.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const fetchSource = app.slice(app.indexOf("  async function backgroundFetch("), app.indexOf("  /** Fix… of a failed fetch"));
  const js = ts.transpile(activitySource + fetchSource, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const calls: string[] = [];
  let reply: { retryAfterMs: number | null } | { updated: number; gaveWay: boolean } = { retryAfterMs: null };
  const globals = {
    $state: (value: unknown) => value,
    getCurrentWindow: () => ({ listen: async () => {} }),
    fetchTarget: () => ({ remote: "origin", label: "origin" }),
    api: {
      fetchInBackground: async (_remote: string, _id: number, auto: boolean) => {
        calls.push(auto ? "auto" : "manual");
        return reply;
      },
      refreshRemoteTags: async () => calls.push("ls-remote"),
    },
    refreshQuietly: async () => calls.push("refresh"),
    confirm: { say: () => calls.push("notice") },
  };
  const { activity, backgroundFetch } = Function(
    ...Object.keys(globals), `${js}; return {activity, backgroundFetch};`,
  )(...Object.values(globals));
  assert.equal(await backgroundFetch(true), null);
  reply = { retryAfterMs: 5000 };
  assert.equal(await backgroundFetch(true), 5000);
  reply = { updated: 0, gaveWay: true };
  await backgroundFetch(true);
  assert.deepEqual(calls, ["auto", "auto", "auto"]);
  assert.deepEqual(activity.recent, []);
  assert.equal(activity.running, null);

  reply = { updated: 1, gaveWay: false };
  await backgroundFetch(false);
  assert.equal(activity.recent[0]?.state, "done");
  assert.deepEqual(calls.slice(3), ["manual", "ls-remote", "refresh", "notice"]);
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
