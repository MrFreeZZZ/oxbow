import { byDay, countsOf, dayOf, keyOf, restoreRequest, sinceWhat, whenSaved } from "../src/lib/localHistory.ts";
import type { LocalVersion } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const version = (more: Partial<LocalVersion> = {}): LocalVersion => ({
  path: "src/app.py",
  id: "0123456789abcdef0123456789abcdef01234567",
  at: 0,
  size: 120,
  base: null,
  fromHead: false,
  additions: 2,
  deletions: 1,
  binary: false,
  kept: true,
  gone: false,
  ...more,
});

const noon = new Date(2026, 9, 29, 12, 0).getTime();
const hours = (n: number) => n * 60 * 60 * 1000;

// The day a version was saved: today, yesterday, or its date.
{
  assert.equal(dayOf(noon - hours(1), noon), "Today");
  assert.equal(dayOf(noon - hours(13), noon), "Yesterday");
  assert.notEqual(dayOf(noon - hours(60), noon), "Yesterday");
  assert.match(whenSaved(noon - hours(1), noon), /^today at /);
  assert.match(whenSaved(noon - hours(13), noon), /^yesterday at /);
  assert.match(whenSaved(noon - hours(60), noon), /^on .+ at /);
}

// The versions stay newest first, under one heading a day.
{
  const list = [version({ at: noon - hours(1) }), version({ at: noon - hours(2) }), version({ at: noon - hours(20) }), version({ at: noon - hours(60) })];
  const days = byDay(list, noon);
  assert.deepEqual(
    days.map((d) => [d.day, d.versions.length]),
    [["Today", 2], ["Yesterday", 1], [dayOf(noon - hours(60), noon), 1]],
  );
  assert.equal(byDay([], noon).length, 0);
}

// What the counts are from: the last commit for the oldest, else the version before; a file new
// since then has nothing before it.
{
  assert.equal(sinceWhat(version({ fromHead: true, base: "abc" })), "since the last commit");
  assert.equal(sinceWhat(version({ base: "abc" })), "since the version before");
  assert.equal(sinceWhat(version()), "a new file");
  assert.deepEqual(countsOf(version()), { add: 2, del: 1 });
  assert.equal(countsOf(version({ additions: null, deletions: null, binary: true })), null);
}

// Two saves of the same bytes are two rows.
assert.notEqual(keyOf(version({ at: 1 })), keyOf(version({ at: 2 })));

// Restore asks, says what it replaces, and is undone from the Operation Log.
{
  const r = restoreRequest(version(), "today at 11:00");
  assert.deepEqual(r.action, { kind: "restoreLocalVersion", path: "src/app.py", id: version().id });
  assert.match(r.title, /^Do you really want to restore app\.py as it was saved today at 11:00\?$/);
  assert.ok(r.body.some((p) => typeof p === "string" && p.includes("is replaced")));
  assert.match(r.note!, /Undo/);
  // A file deleted since comes back.
  const back = restoreRequest(version({ gone: true }), "yesterday at 18:05");
  assert.ok(back.body.some((p) => typeof p === "string" && p.includes("back on disk")));
}
