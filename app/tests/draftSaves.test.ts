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
  // A 1 is being written, held by the backend.
  await tick();
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

/** A backend whose calls each wait for the test to let them go, failing or not. */
function stepped() {
  const writes: string[] = [];
  const pending: { what: string; go: (fail?: string) => void }[] = [];
  const call = (what: string) =>
    new Promise<void>((resolve, reject) =>
      pending.push({ what, go: (fail) => (fail ? reject(fail) : (writes.push(what), resolve())) }),
    );
  const api: DraftApi = {
    saveConflictDraft: (workdir, d) => call(`save ${workdir} ${d.step}`),
    dropConflictDraft: (workdir) => call(`drop ${workdir}`),
  };
  /** Let the oldest call waiting go, once it has started. */
  const next = async (fail?: string) => {
    while (!pending.length) await tick();
    pending.shift()!.go(fail);
    await tick();
  };
  return { api, writes, pending, next };
}

// Try Again while a newer save is being written: the old one that failed isn't written after it,
// and closing finds everything written.
{
  const b = stepped();
  const saves = new DraftSaves(b.api);
  saves.later("/src/A", () => draft("A1"), 0);
  void saves.flush();
  await b.next("disk busy");
  saves.later("/src/A", () => draft("A2"), 0);
  void saves.flush();
  while (!b.pending.length) await tick();
  saves.retry();
  const closing = saves.settle(1000);
  await b.next();
  assert.equal(await closing, null);
  assert.deepEqual(b.writes, ["save /src/A A2"]);
  assert.equal(b.pending.length, 0);
}

// Try Again while a newer save waits in the queue behind another folder's: still only the newer.
{
  const b = stepped();
  const saves = new DraftSaves(b.api);
  saves.later("/src/A", () => draft("A1"), 0);
  void saves.flush();
  await b.next("disk busy");
  saves.later("/src/B", () => draft("B1"), 0);
  void saves.flush();
  saves.later("/src/A", () => draft("A2"), 0);
  void saves.flush();
  saves.retry();
  await b.next();
  await b.next();
  assert.equal(await saves.settle(1000), null);
  assert.deepEqual(b.writes, ["save /src/B B1", "save /src/A A2"]);
}

// A drop that failed doesn't come back over the draft saved after it.
{
  const b = stepped();
  const saves = new DraftSaves(b.api);
  saves.later("/src/A", () => null, 0);
  void saves.flush();
  await b.next("disk busy");
  saves.later("/src/A", () => draft("A2"), 0);
  void saves.flush();
  await b.next();
  saves.retry();
  assert.equal(await saves.settle(1000), null);
  assert.deepEqual(b.writes, ["save /src/A A2"]);
}

// Closing again and again during a slow save adds nothing after it.
{
  const b = stepped();
  const saves = new DraftSaves(b.api);
  saves.later("/src/A", () => draft("A1"), 0);
  void saves.flush();
  await b.next("disk busy");
  saves.retry();
  saves.retry();
  assert.match((await saves.settle(5)) ?? "", /still being written/);
  saves.retry();
  await b.next();
  assert.equal(await saves.settle(1000), null);
  assert.deepEqual(b.writes, ["save /src/A A1"]);
  assert.equal(b.pending.length, 0);
}

// The newest save failing is what closing names, and Try Again writes that one.
{
  const b = stepped();
  const saves = new DraftSaves(b.api);
  saves.later("/src/A", () => draft("A1"), 0);
  void saves.flush();
  saves.later("/src/A", () => draft("A2"), 0);
  void saves.flush();
  // A1 is skipped: A2 is the first and only call, and it fails.
  await b.next("disk full");
  assert.equal(await saves.settle(1000), "disk full");
  saves.retry();
  await b.next();
  assert.equal(await saves.settle(1000), null);
  assert.deepEqual(b.writes, ["save /src/A A2"]);
}

// Nothing waiting: closing doesn't wait.
{
  assert.equal(await new DraftSaves(backend().api).settle(1000), null);
}
