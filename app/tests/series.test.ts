import { baseLine, firstPick, interRows, pairLabel, stepQuery, tally, wordDiff } from "../src/lib/series.ts";
import type { InterLine, OpEntry, Series, SeriesPair } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const l = (outer: InterLine["outer"], text: string, inner: InterLine["inner"] = "added"): InterLine => ({ outer, inner, text });

// Unchanged lines face themselves; an old run faces the new run after it.
{
  const both = l("both", "a", "context");
  const rows = interRows([l("both", "fn", "hunk"), both, l("old", "x"), l("old", "y"), l("new", "X"), both, l("new", "z")]);
  assert.deepEqual(
    rows.map((r) => (r.kind === "across" ? `~${r.line.text}` : `${r.left?.text ?? "-"}|${r.right?.text ?? "-"}`)),
    ["~fn", "a|a", "x|X", "y|-", "a|a", "-|z"],
  );
  // A new line before an old one: two rows, not one pair.
  const crossed = interRows([l("new", "n"), l("old", "o")]);
  assert.deepEqual(
    crossed.map((r) => (r.kind === "pair" ? `${r.left?.text ?? "-"}|${r.right?.text ?? "-"}` : "")),
    ["-|n", "o|-"],
  );
}

// Changed words are marked on each side, and spaces between changed words join them.
{
  const [a, b] = wordDiff("let total = count + 1;", "let total = count * 2;");
  assert.deepEqual(
    a.filter((p) => p.changed).map((p) => p.text),
    ["+ 1"],
  );
  assert.deepEqual(
    b.filter((p) => p.changed).map((p) => p.text),
    ["* 2"],
  );
  assert.equal(a.map((p) => p.text).join(""), "let total = count + 1;");
  const [same] = wordDiff("same", "same");
  assert.deepEqual(same, [{ text: "same", changed: false }]);
  const long = "x ".repeat(500);
  assert.equal(wordDiff(long, "y")[0][0].changed, true);
}

const pair = (kind: SeriesPair["kind"], flags: Partial<SeriesPair> = {}): SeriesPair => ({
  kind,
  old: null,
  new: null,
  oldIndex: null,
  newIndex: null,
  message: false,
  code: false,
  author: false,
  sections: [],
  ...flags,
});

{
  assert.equal(pairLabel(pair("same")), "Same");
  assert.equal(pairLabel(pair("changed", { message: true })), "Message changed");
  assert.equal(pairLabel(pair("changed", { message: true, code: true })), "Message and code changed");
  assert.equal(pairLabel(pair("changed", { message: true, code: true, author: true })), "Message, code and author changed");
  assert.equal(pairLabel(pair("dropped")), "Dropped");
}

{
  const series: Series = {
    oldTip: "a",
    newTip: "b",
    oldBase: "1111111",
    newBase: "2222222",
    baseName: "main",
    baseCommits: 3,
    oldCount: 2,
    newCount: 3,
    pairs: [pair("same"), pair("added"), pair("changed", { code: true })],
    command: "git range-diff",
  };
  assert.equal(firstPick(series), 2);
  assert.equal(firstPick({ ...series, pairs: [pair("same"), pair("dropped")] }), 1);
  assert.equal(baseLine(series), "Moved onto main: 3 newer commits under the series.");
  assert.deepEqual(
    tally(series).map((t) => t.text),
    ["1 changed", "1 new", "1 the same"],
  );
  assert.equal(baseLine({ ...series, newBase: "1111111", baseCommits: 0 }), "Same base: only the commits themselves were rewritten.");
  assert.equal(firstPick({ ...series, pairs: [pair("same")] }), 0);
}

// A step of the Operation Log: its rewritten branches, the checked-out one first.
{
  const entry = {
    rewrites: ["refs/heads/lower", "refs/heads/upper"],
    refs: [
      { name: "refs/heads/lower", before: "l1", after: "l2" },
      { name: "refs/heads/upper", before: "u1", after: "u2" },
      { name: "refs/heads/other", before: "o1", after: "o2" },
    ],
    headBefore: { branch: "upper", commit: "u1" },
    headAfter: { branch: "upper", commit: "u2" },
  } as unknown as OpEntry;
  const q = stepQuery(entry, "Edit stack")!;
  assert.equal(q.branch, "upper");
  assert.deepEqual([q.old, q.new], ["u1", "u2"]);
  assert.equal(q.branches.length, 2);
  assert.deepEqual(q.rewritten, ["refs/heads/lower", "refs/heads/upper", "refs/heads/other"]);
  assert.equal(stepQuery(entry, "x", "lower")!.old, "l1");
  assert.equal(stepQuery({ ...entry, rewrites: [] }, "x"), null);
  assert.equal(stepQuery({ ...entry, rewrites: null }, "x"), null);
}
