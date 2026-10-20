import { afterClear, dayOf, duration, entryText, exitOf, outputOpen, picked, shows, stamp, trimmed, upsert, type ConsoleEntry, type ConsoleRan } from "../src/lib/console.ts";
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

// An update of a running entry that comes after the log has it done changes nothing.
{
  const done = entry(1, { commands: [ran("git push")] });
  const list = upsert([done], entry(1, { running: true }));
  assert.equal(list[0].running, undefined);
  assert.equal(upsert(list, entry(1, { title: "Push", millis: 20 }))[0].millis, 20);
}

// The live list keeps what the log keeps: per log, reads first to go, what runs always stays.
{
  type Of = ConsoleEntry & { log: string };
  const of = (id: number, log: string, extra: Partial<ConsoleEntry> = {}): Of => ({ ...entry(id, extra), log });
  let list: Of[] = [of(0, "a", { running: true })];
  for (let i = 1; i <= 1100; i++) list = upsert(list, of(i, "a", { kind: "read" }));
  list = upsert(list, of(2000, "b", { kind: "read" }));
  const kept = trimmed(list, (e) => e.log);
  assert.equal(kept.filter((e) => e.log === "a" && e.kind === "read").length, 300);
  assert.equal(kept[0].id, 0, "what runs stays");
  assert.equal(kept[1].id, 801);
  assert.ok(kept.some((e) => e.log === "b"));
  let actions: Of[] = [];
  for (let i = 1; i <= 1200; i++) actions.push(of(i, "a"));
  assert.equal(trimmed(actions, (e) => e.log).length, 1000);
  assert.equal(trimmed(actions, (e) => e.log)[0].id, 201);
}

// Clear History drops what came for the cleared logs while a load was on its way, except what runs.
{
  const pending = [
    { log: "a", entry: entry(1) },
    { log: "a", entry: entry(2, { running: true }) },
    { log: "b", entry: entry(3) },
  ];
  assert.deepEqual(afterClear(pending, ["a"]).map((p) => p.entry.id), [2, 3]);
  assert.deepEqual(afterClear(pending, ["a", "b"]).map((p) => p.entry.id), [2]);
}

// An entry that started and finished before the clear leaves nothing behind, not its running update.
{
  const pending = [
    { log: "a", entry: entry(44, { running: true }) },
    { log: "a", entry: entry(44) },
    { log: "a", entry: entry(45, { running: true }) },
    { log: "b", entry: entry(46) },
  ];
  assert.deepEqual(afterClear(pending, ["a"]).map((p) => p.entry.id), [45, 46]);
}

// Picking entries: a click picks one, Shift the run from the last click, ⌘ adds or takes one away.
{
  const order = ["a", "b", "c", "d", "e"];
  const plain = { range: false, toggle: false };
  assert.deepEqual([...picked(order, new Set(["a", "b"]), "a", "c", plain)], ["c"]);
  assert.deepEqual([...picked(order, new Set(["b"]), "b", "d", { range: true, toggle: false })], ["b", "c", "d"]);
  assert.deepEqual([...picked(order, new Set(["d"]), "d", "b", { range: true, toggle: false })], ["b", "c", "d"]);
  assert.deepEqual([...picked(order, new Set(["a", "c"]), "c", "a", { range: false, toggle: true })], ["c"]);
  assert.deepEqual([...picked(order, new Set(["a"]), "a", "e", { range: false, toggle: true })], ["a", "e"]);
  // Shift with nothing to start from picks the one clicked.
  assert.deepEqual([...picked(order, new Set(), null, "c", { range: true, toggle: false })], ["c"]);
  // ⌘⇧ adds the run to what is picked.
  assert.deepEqual([...picked(order, new Set(["a"]), "c", "e", { range: true, toggle: true })].sort(), ["a", "c", "d", "e"]);
}

// Copied text: time, title and how it ended, then every command with its output.
{
  const time = new Date(2026, 9, 20, 7, 12, 3).getTime();
  assert.equal(stamp(time), "2026-10-20 07:12:03");
  const push = entry(5, {
    time,
    title: "Push main",
    failed: true,
    millis: 1200,
    commands: [
      ran("git diff --cached", { role: "secrets check", output: [] }),
      ran("git push origin main", { code: 1, output: [{ t: "! [rejected] main -> main (fetch first)", e: true }] }),
    ],
  });
  assert.equal(entryText(push), ["2026-10-20 07:12:03  ·  Push main  ·  exit 1", "[secrets check] % git diff --cached", "% git push origin main · exit 1", "  ! [rejected] main -> main (fetch first)"].join("\n"));
  const read = entry(6, { time, kind: "read", title: "Read by Oxbow", gitoxide: true, millis: 4, note: "Read the history" });
  assert.equal(entryText(read, "oxbow"), ["2026-10-20 07:12:03  ·  Read by Oxbow  ·  oxbow  ·  gitoxide · no git process  ·  4 ms", "Read the history"].join("\n"));
}
