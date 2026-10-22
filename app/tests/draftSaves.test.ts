import { DraftSaves, type DraftApi, type DraftNote } from "../src/lib/draftSaves.ts";
import type { Draft } from "../src/lib/drafts.ts";
import assert from "node:assert/strict";

const tick = (ms = 1) => new Promise((resolve) => setTimeout(resolve, ms));
const draft = (step: string) => ({ version: 1, step, savedAt: 0, seen: [], files: [], old: [] }) as Draft;

/** The backend as the saves call it: every call with its folder, and a way to hold or fail them. */
function backend() {
  const calls: string[] = [];
  let hold: Promise<void> | null = null;
  let fail: string | null = null;
  const api: DraftApi = {
    async saveConflictDraft(workdir, d) {
      if (hold) await hold;
      if (fail) throw fail;
      calls.push(`save ${workdir} ${d.step}`);
    },
    async dropConflictDraft(workdir) {
      if (hold) await hold;
      if (fail) throw fail;
      calls.push(`drop ${workdir}`);
    },
  };
  return {
    api,
    calls,
    hold() {
      let release!: () => void;
      hold = new Promise<void>((resolve) => (release = resolve));
      return () => ((hold = null), release());
    },
    fail: (why: string | null) => (fail = why),
  };
}

// Only the last save made while typing goes on runs, once typing pauses.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  for (const step of ["a", "b", "c"]) saves.later("/src/A", () => draft(step), 5);
  assert.deepEqual(b.calls, []);
  await tick(20);
  assert.deepEqual(b.calls, ["save /src/A c"]);
}

// Flush starts the waiting save at once and resolves when it is written, not before.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  const notes: DraftNote["kind"][] = [];
  saves.later("/src/A", () => draft("last edit"), 300, (note) => notes.push(note.kind));
  await saves.flush();
  assert.deepEqual(b.calls, ["save /src/A last edit"]);
  assert.deepEqual(notes, ["saved"]);
}

// Nothing left to keep removes the folder's draft.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  saves.later("/src/A", () => null, 0);
  await saves.flush();
  assert.deepEqual(b.calls, ["drop /src/A"]);
}

// A save made for A while an earlier one is held still goes to A after the window opened B,
// and B's own draft is saved after it, untouched by A's.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  const release = b.hold();
  saves.later("/src/A", () => draft("A 1"), 0);
  void saves.flush();
  saves.later("/src/A", () => draft("A 2"), 300);
  void saves.flush();
  saves.later("/src/B", () => draft("B 1"), 0);
  const done = saves.flush();
  await tick();
  assert.deepEqual(b.calls, []);
  release();
  await done;
  assert.deepEqual(b.calls, ["save /src/A A 1", "save /src/A A 2", "save /src/B B 1"]);
}

// A failed save is told, kept, and named when closing; Try Again writes it.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  const notes: DraftNote[] = [];
  b.fail("Can’t save the draft: disk full.");
  saves.later("/src/A", () => draft("A"), 300, (note) => notes.push(note));
  assert.equal(await saves.settle(1000), "Can’t save the draft: disk full.");
  assert.deepEqual(notes, [{ kind: "failed", why: "Can’t save the draft: disk full." }]);
  // Still failing: still not saved.
  saves.retry();
  assert.equal(await saves.settle(1000), "Can’t save the draft: disk full.");
  b.fail(null);
  saves.retry();
  assert.equal(await saves.settle(1000), null);
  assert.deepEqual(b.calls, ["save /src/A A"]);
  assert.equal(notes.at(-1)?.kind, "saved");
}

// Another folder's save going through doesn't stand for the one that failed; a newer save of
// the same folder does.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  b.fail("disk full");
  saves.later("/src/A", () => draft("A 1"), 0);
  await saves.flush();
  b.fail(null);
  saves.later("/src/B", () => draft("B 1"), 0);
  assert.equal(await saves.settle(1000), "disk full");
  saves.later("/src/A", () => draft("A 2"), 0);
  assert.equal(await saves.settle(1000), null);
  assert.deepEqual(b.calls, ["save /src/B B 1", "save /src/A A 2"]);
}

// A save slower than the wait is not taken for written.
{
  const b = backend();
  const saves = new DraftSaves(b.api);
  const release = b.hold();
  saves.later("/src/A", () => draft("A"), 300);
  assert.match((await saves.settle(10)) ?? "", /still being written/);
  release();
  assert.equal(await saves.settle(1000), null);
}

// Nothing waiting: closing doesn't wait.
{
  assert.equal(await new DraftSaves(backend().api).settle(1000), null);
}
