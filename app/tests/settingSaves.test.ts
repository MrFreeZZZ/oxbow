import { isNewer, loadedOver, SaveQueue } from "../src/lib/settingSaves.ts";
import assert from "node:assert/strict";

// A change is taken only when it is newer than what the window has for that key.
{
  const revisions = { "oxbow.diff.wrap": 7 };
  assert.equal(isNewer(revisions, 3, "oxbow.diff.wrap", 6), false);
  assert.equal(isNewer(revisions, 3, "oxbow.diff.wrap", 8), true);
  // A key without its own number goes by when settings.json was loaded.
  assert.equal(isNewer(revisions, 3, "oxbow.text.fontSize", 3), false);
  assert.equal(isNewer(revisions, 3, "oxbow.text.fontSize", 4), true);
  // Before the load every change is newer.
  assert.equal(isNewer({}, -1, "oxbow.text.fontSize", 0), true);
}

// Loading settings.json keeps keys that changed after it was read, and drops keys it doesn't
// hold that didn't.
{
  const now = { "oxbow.diff.wrap": false, "oxbow.theme": "dracula", "oxbow.text.fontSize": 14 };
  const revisions = { "oxbow.diff.wrap": 5, "oxbow.theme": 2 };
  const out = loadedOver(now, revisions, 4, { "oxbow.diff.wrap": true, "oxbow.appearance": "dark" });
  assert.deepEqual(out, { "oxbow.diff.wrap": false, "oxbow.appearance": "dark" });
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 1));

// Saves made before any could start: only the last one runs.
{
  const queue = new SaveQueue();
  const ran: string[] = [];
  const all = await Promise.all(["a", "b", "c"].map((v) => queue.run("k", async () => (ran.push(v), v), "skipped")));
  assert.deepEqual(all, ["skipped", "skipped", "c"]);
  assert.deepEqual(ran, ["c"]);
}

// Saves of one key run one after another, in order; a save replaced before its turn is skipped.
{
  const queue = new SaveQueue();
  const ran: string[] = [];
  let release!: () => void;
  const first = queue.run(
    "k",
    async () => {
      await new Promise<void>((resolve) => (release = resolve));
      ran.push("a");
      return "a";
    },
    "skipped",
  );
  await tick();
  const second = queue.run("k", async () => (ran.push("b"), "b"), "skipped");
  const third = queue.run("k", async () => (ran.push("c"), "c"), "skipped");
  // Another key doesn't wait.
  const other = await queue.run("x", async () => "x", "skipped");
  assert.equal(other, "x");
  await tick();
  assert.deepEqual(ran, []);
  release();
  assert.equal(await first, "a");
  assert.equal(await second, "skipped");
  assert.equal(await third, "c");
  assert.deepEqual(ran, ["a", "c"]);
}

// A save that fails doesn't hold up the next one, and knows whether a newer one came meanwhile.
{
  const queue = new SaveQueue();
  let newestWhenDone: boolean | null = null;
  let fail!: () => void;
  const first = queue.run(
    "k",
    async () => {
      await new Promise<void>((_, reject) => (fail = () => reject(new Error("disk full"))));
      return "a";
    },
    "skipped",
  ).catch((err: Error) => err.message);
  await tick();
  const second = queue.run(
    "k",
    async (newest) => {
      newestWhenDone = newest();
      return "b";
    },
    "skipped",
  );
  fail();
  assert.equal(await first, "disk full");
  assert.equal(await second, "b");
  assert.equal(newestWhenDone, true);
}

// The save that is running learns that a newer one was made while it ran.
{
  const queue = new SaveQueue();
  let release!: () => void;
  let wasNewest: boolean | null = null;
  const first = queue.run(
    "k",
    async (newest) => {
      await new Promise<void>((resolve) => (release = resolve));
      wasNewest = newest();
      return "a";
    },
    "skipped",
  );
  await tick();
  void queue.run("k", async () => "b", "skipped");
  release();
  await first;
  assert.equal(wasNewest, false);
}
