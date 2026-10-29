import { Latest } from "../src/lib/latest.ts";
import assert from "node:assert/strict";

/** A request whose reply comes when the test says. */
function deferred<T>() {
  let reply!: (value: T) => void;
  const promise = new Promise<T>((resolve) => (reply = resolve));
  return { promise, reply };
}

/** The list as Repository Files keeps it: each load asks, and only the newest reply is shown. */
function list() {
  const state = { repo: "A" as string | null, shown: null as string | null };
  const latest = new Latest(() => state.repo);
  return {
    state,
    latest,
    async load(request: Promise<string>) {
      const current = latest.begin();
      const value = await request;
      if (current()) state.shown = value;
    },
    /** Another repository: what was shown for the last one goes. */
    open(repo: string) {
      state.repo = repo;
      state.shown = null;
      latest.drop();
    },
  };
}

// A → B → A: the first reply for A, come last, isn't shown over the newest one.
{
  const l = list();
  const first = deferred<string>();
  const onA = l.load(first.promise);
  l.open("B");
  const second = deferred<string>();
  const onB = l.load(second.promise);
  l.open("A");
  const third = deferred<string>();
  const again = l.load(third.promise);
  third.reply("A, read again");
  await again;
  second.reply("B");
  await onB;
  first.reply("A, read first");
  await onA;
  assert.equal(l.state.shown, "A, read again");
}

// Two loads of one repository: the older reply, come after the newer, is dropped.
{
  const l = list();
  const older = deferred<string>();
  const newer = deferred<string>();
  const a = l.load(older.promise);
  const b = l.load(newer.promise);
  newer.reply("new");
  await b;
  older.reply("old");
  await a;
  assert.equal(l.state.shown, "new");
}

// A reply that comes after the repository changed is dropped even with no load since.
{
  const l = list();
  const asked = deferred<string>();
  const done = l.load(asked.promise);
  l.state.repo = "B";
  asked.reply("A");
  await done;
  assert.equal(l.state.shown, null);
}

// The newest reply is shown when it comes last, as it usually does.
{
  const l = list();
  await l.load(Promise.resolve("first"));
  await l.load(Promise.resolve("second"));
  assert.equal(l.state.shown, "second");
}

// Without a key, only the order counts.
{
  const latest = new Latest();
  const one = latest.begin();
  assert.equal(one(), true);
  const two = latest.begin();
  assert.equal(one(), false);
  assert.equal(two(), true);
  latest.drop();
  assert.equal(two(), false);
}
