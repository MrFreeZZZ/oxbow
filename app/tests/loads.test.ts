import { Reloads, Tickets, type ReloadGate } from "../src/lib/loads.ts";
import assert from "node:assert/strict";

/** A reload whose answer comes when the test says. */
function deferred() {
  let done!: () => void;
  const promise = new Promise<void>((resolve) => (done = resolve));
  return { promise, done };
}

/** Reloads as the history loader keeps them: each run waits for the test to finish it. */
function reloads() {
  const state = { gate: "on" as ReloadGate, runs: [] as ReturnType<typeof deferred>[] };
  const queue = new Reloads(
    () => {
      const run = deferred();
      state.runs.push(run);
      return run.promise;
    },
    () => state.gate,
  );
  return { state, queue };
}

// An answer counts only for the repository it was asked of: A → B → A drops both older ones.
{
  const tickets = new Tickets();
  const onA = tickets.switch();
  const reload = tickets.take();
  const onB = tickets.switch();
  assert.equal(tickets.current(onA), false);
  assert.equal(tickets.current(reload), false);
  assert.equal(tickets.current(onB), true);
  const againA = tickets.switch();
  assert.equal(tickets.current(onA), false);
  assert.equal(tickets.current(onB), false);
  assert.equal(tickets.current(againA), true);
  // Requests of one repository count alike, each with its own number.
  const page = tickets.take();
  assert.equal(tickets.current(page), true);
  assert.notEqual(page.requestId, againA.requestId);
}

// Asked for while one runs: one more follows it, for everyone who asked.
{
  const { state, queue } = reloads();
  const first = queue.ask();
  assert.equal(state.runs.length, 1);
  const second = queue.ask();
  const third = queue.ask();
  assert.equal(second, third);
  assert.equal(state.runs.length, 1);
  state.runs[0].done();
  await first;
  // The one that follows starts once the first is done.
  await new Promise((done) => setTimeout(done, 0));
  assert.equal(state.runs.length, 2);
  state.runs[1].done();
  await second;
  assert.equal(queue.running, null);
  assert.equal(state.runs.length, 2);
}

// No repository, or one opening: nothing runs.
{
  const { state, queue } = reloads();
  state.gate = "off";
  await queue.ask();
  assert.equal(state.runs.length, 0);
}

// Older commits on their way: the reload follows them, once, with the new limit.
{
  const { state, queue } = reloads();
  state.gate = "paging";
  await queue.ask();
  await queue.ask();
  assert.equal(state.runs.length, 0);
  state.gate = "on";
  queue.paged();
  assert.equal(state.runs.length, 1);
  state.runs[0].done();
  queue.paged();
  assert.equal(state.runs.length, 1);
}

// Paging stopped for another repository: the reload asked for meanwhile doesn't come.
{
  const { state, queue } = reloads();
  state.gate = "paging";
  await queue.ask();
  queue.forgetAfterPage();
  state.gate = "on";
  queue.paged();
  assert.equal(state.runs.length, 0);
}
