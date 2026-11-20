import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import ts from "typescript";

test("a skipped timer tick leaves Activity and remote tags alone", async () => {
  const activitySource = fs.readFileSync(new URL("../src/lib/activity.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const app = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");
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
