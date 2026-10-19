import { dayOf, duration, exitOf, outputOpen, shows, upsert, type ConsoleEntry, type ConsoleRan } from "../src/lib/console.ts";
import assert from "node:assert/strict";

const ran = (display: string, extra: Partial<ConsoleRan> = {}): ConsoleRan => ({ display, time: 0, millis: 5, code: 0, stopped: false, ...extra });
const entry = (id: number, extra: Partial<ConsoleEntry> = {}): ConsoleEntry => ({
  id,
  time: id,
  kind: "action",
  title: "Commit",
  millis: 10,
  failed: false,
  stopped: false,
  gitoxide: false,
  commands: [],
  ...extra,
});

// The filters: My Actions is what the user started; Background the timer's fetches and the reads.
{
  const commit = entry(1, { commands: [ran("git commit -m 'Add the console'", { output: [{ t: "[main 7c1e9a2] Add the console" }] })] });
  const fetch = entry(2, { kind: "background", title: "Auto fetch" });
  const read = entry(3, { kind: "read", title: "Read the repository again", note: "refs 41 · commits 1 240" });
  assert.deepEqual([commit, fetch, read].filter((e) => shows(e, "mine", "")).map((e) => e.id), [1]);
  assert.deepEqual([commit, fetch, read].filter((e) => shows(e, "background", "")).map((e) => e.id), [2, 3]);
  // The search looks at titles, commands, their output and the read's note.
  assert.ok(shows(commit, "all", "7C1E9A2"));
  assert.ok(shows(commit, "all", "the console"));
  assert.ok(shows(read, "all", "refs 41"));
  assert.ok(!shows(fetch, "all", "commit"));
}

// A live update replaces the running entry; a new one goes in time order.
{
  const list = [entry(1), entry(5)];
  assert.deepEqual(upsert(list, entry(3)).map((e) => e.id), [1, 3, 5]);
  const done = upsert(list, { ...entry(5), title: "Push main" });
  assert.equal(done.length, 2);
  assert.equal(done[1].title, "Push main");
}

// The exit code shown on a failed entry is of its own command, not of a check before it.
{
  const push = entry(1, {
    failed: true,
    commands: [ran("git rev-parse --verify origin/main", { role: "check", code: 128 }), ran("git push origin main", { code: 1 })],
  });
  assert.equal(exitOf(push), 1);
  assert.equal(exitOf(entry(2, { failed: true })), null);
}

// Output shows at once when it went wrong or is a short one of an action.
{
  const lines = (n: number) => Array.from({ length: n }, (_, i) => ({ t: `line ${i}` }));
  assert.ok(outputOpen(entry(1), ran("git commit", { output: lines(3) })));
  assert.ok(!outputOpen(entry(1), ran("git push", { output: lines(30) })));
  assert.ok(outputOpen(entry(1), ran("git push", { output: lines(30), code: 1 })));
  assert.ok(!outputOpen(entry(1, { kind: "background" }), ran("git fetch", { output: lines(2) })));
  assert.ok(!outputOpen(entry(1), ran("git add")));
}

assert.equal(duration(38), "38 ms");
assert.equal(duration(2140), "2.1 s");
assert.equal(duration(42_000), "42 s");
assert.equal(duration(64_000), "1 min 4 s");

{
  const now = new Date(2026, 9, 19, 17, 0).getTime();
  assert.equal(dayOf(new Date(2026, 9, 19, 0, 5).getTime(), now), "Today");
  assert.equal(dayOf(new Date(2026, 9, 18, 23, 59).getTime(), now), "Yesterday");
  assert.equal(dayOf(new Date(2026, 9, 13, 9, 0).getTime(), now), "Tuesday, 13 October");
  assert.equal(dayOf(new Date(2025, 9, 13, 9, 0).getTime(), now), "Monday, 13 October 2025");
}
