import { DraftSaves } from "../src/lib/draftSaves.ts";
import assert from "node:assert/strict";

const tick = (ms = 1) => new Promise((resolve) => setTimeout(resolve, ms));

// Only the last save made while typing goes on runs, once typing pauses.
{
  const saves = new DraftSaves();
  const ran: string[] = [];
  for (const v of ["a", "b", "c"]) saves.later(async () => void ran.push(v), 5);
  assert.deepEqual(ran, []);
  await tick(20);
  assert.deepEqual(ran, ["c"]);
}

// Flush starts the waiting save at once and resolves when it is written, not before.
{
  const saves = new DraftSaves();
  const written: string[] = [];
  saves.later(async () => {
    await tick(5);
    written.push("last edit");
  }, 300);
  await saves.flush();
  assert.deepEqual(written, ["last edit"]);
}

// A save waits for the one before it, even one made by a screen that is gone; flush waits for both.
{
  const saves = new DraftSaves();
  const order: string[] = [];
  let release!: () => void;
  saves.later(() => new Promise<void>((resolve) => (release = () => (order.push("first"), resolve()))), 0);
  void saves.flush();
  saves.later(async () => void order.push("second"), 300);
  const done = saves.flush();
  await tick();
  assert.deepEqual(order, []);
  release();
  await done;
  assert.deepEqual(order, ["first", "second"]);
}

// A save that fails doesn't stop the ones after it.
{
  const saves = new DraftSaves();
  const ran: string[] = [];
  saves.later(() => Promise.reject(new Error("disk full")), 0);
  void saves.flush();
  saves.later(async () => void ran.push("next"), 0);
  await saves.flush();
  assert.deepEqual(ran, ["next"]);
}

// Nothing waiting: flush resolves at once.
{
  await new DraftSaves().flush();
}
