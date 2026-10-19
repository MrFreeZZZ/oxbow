import { CountTurns } from "../src/lib/counting.ts";
import assert from "node:assert/strict";

// His order: A asked, then B; B's page comes and its count starts; A's page fails late.
{
  const t = new CountTurns();
  const a = t.read("A", false);
  const b = t.read("B", false);
  assert.equal(t.start("B", b, "B"), true);
  // A's late answer (a page or an error) starts nothing.
  assert.equal(t.start("A", a, "B"), false);
  t.end(a); // nothing of A's
  assert.equal(t.start("B", b, "B"), false); // B's count already runs
  // A tab opened while B is counted does not ask again.
  const tab = t.read("B", false);
  assert.equal(t.start("B", tab, "B"), false);
  t.end(b);
}
// A count that failed or was stopped is asked for again by the next read.
{
  const t = new CountTurns();
  const r = t.read("B", false);
  assert.equal(t.start("B", r, "B"), true);
  t.end(r);
  const again = t.read("B", false);
  assert.equal(t.start("B", again, "B"), true);
}
// A newer read for the same key, before the first page came, counts in its place.
{
  const t = new CountTurns();
  const first = t.read("B", false);
  const tab = t.read("B", false);
  assert.equal(t.start("B", tab, "B"), true);
  assert.equal(t.start("B", first, "B"), false);
}
// Already counted: no read counts.
{
  const t = new CountTurns();
  const r = t.read("B", true);
  assert.equal(t.start("B", r, "B"), false);
}
// Back to A after B: A counts again (its earlier count was stopped by B's).
{
  const t = new CountTurns();
  const a = t.read("A", false);
  assert.equal(t.start("A", a, "A"), true);
  const b = t.read("B", false);
  assert.equal(t.start("B", b, "B"), true);
  t.end(a); // A's count stopped by B's: B keeps the turn
  assert.equal(t.start("B", b, "B"), false);
  const a2 = t.read("A", false);
  assert.equal(t.start("A", a2, "A"), true);
}
