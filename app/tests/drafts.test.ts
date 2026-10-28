import { hasWork, makeDraft, mergedCount, readDraft, takeDraft, type DraftState } from "../src/lib/drafts.ts";
import { resultOf, type Sides } from "../src/lib/conflicts.ts";
import type { Chunk, ConflictFile } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const sides: Sides = { left: "theirs", right: "ours", leftLabel: "feature", rightLabel: "main" };
const chunks: Chunk[] = [
  { kind: "same", lines: ["one"] },
  { kind: "conflict", ours: ["2 on main"], base: ["two"], theirs: ["TWO on feature"] },
];
const file = (over: Partial<ConflictFile> = {}): ConflictFile => ({
  path: "a.txt",
  base: true,
  ours: true,
  theirs: true,
  binary: false,
  link: false,
  text: true,
  renamed: null,
  chunks,
  openEnd: false,
  stages: "1:100644:aa 2:100644:bb 3:100644:cc",
  ...over,
});

const empty = (): DraftState => ({ choices: new Map(), texts: new Map(), stages: new Map(), results: new Map(), old: new Map(), seen: [], merged: new Map() });
/** Saved and read back the way the backend does: as JSON. */
const roundTrip = (value: unknown) => readDraft(JSON.parse(JSON.stringify(value)));

// Nothing decided or typed is nothing to keep.
{
  const state = empty();
  state.choices.set("a.txt", [null]);
  state.stages.set("a.txt", file().stages);
  assert.equal(hasWork([null], null), false);
  assert.equal(makeDraft("merge:x:y", state, ["a.txt"], 1), null);
}

// Decisions, typed lines and text come back exactly: an empty file, Windows line ends, a last
// line with no line break, a conflict typed as no lines and one typed as one empty line.
{
  const state = empty();
  const texts = {
    "empty.txt": { text: "", start: "one\n", stray: 0 },
    "crlf.txt": { text: "one\r\ntwo\r\n", start: "one\r\n", stray: 0 },
    "open.txt": { text: "one\ntwo", start: "one", stray: 0 },
  };
  for (const [path, text] of Object.entries(texts)) {
    state.texts.set(path, text);
    state.choices.set(path, [null]);
    state.stages.set(path, `stages of ${path}`);
    state.results.set(path, text.text);
  }
  state.choices.set("__proto__", [{ lines: [] }, { lines: [""] }, "both"]);
  state.stages.set("__proto__", "s");
  state.seen = ["done.txt", ...Object.keys(texts), "__proto__"];
  const conflicted = [...Object.keys(texts), "__proto__", "untouched.txt"];
  const draft = roundTrip(makeDraft("merge:x:y", state, conflicted, 1000));
  assert.ok(draft);
  assert.deepEqual(
    draft.files.map((f) => f.path),
    ["empty.txt", "crlf.txt", "open.txt", "__proto__"],
  );
  // Text is saved once, not again as the result.
  assert.deepEqual(draft.files.slice(0, 3).map((f) => f.result), ["", "", ""]);
  const back = takeDraft(draft, "merge:x:y", conflicted);
  for (const [path, text] of Object.entries(texts)) {
    assert.deepEqual(back.texts.get(path), text);
    assert.equal(back.results.get(path), text.text);
    assert.equal(back.stages.get(path), `stages of ${path}`);
  }
  assert.equal(back.texts.get("empty.txt")!.text, "");
  assert.deepEqual(back.choices.get("__proto__"), [{ lines: [] }, { lines: [""] }, "both"]);
  assert.deepEqual(back.seen, [...state.seen, "untouched.txt"]);
  assert.equal(back.old.size, 0);
  // In another step the text is what is kept to compare.
  const other = takeDraft(draft, "merge:x:z", conflicted);
  assert.equal(other.old.get("crlf.txt")!.result, "one\r\ntwo\r\n");
}

// Conflicts Oxbow merged stay counted for a file once it is resolved, also after a restart.
{
  const state = empty();
  state.choices.set("done.txt", [{ lines: ["A", "B"], merged: true }, "left", { lines: ["x"], merged: true }]);
  state.choices.set("open.txt", [{ lines: ["typed"] }, null]);
  state.stages.set("open.txt", "s");
  state.seen = ["done.txt", "open.txt", "gone.txt"];
  state.merged.set("gone.txt", 1);
  assert.equal(mergedCount(state, "done.txt"), 2);
  assert.equal(mergedCount(state, "open.txt"), 0);
  const draft = roundTrip(makeDraft("merge:x:y", state, ["open.txt"], 1));
  assert.deepEqual(draft?.merged, [["done.txt", 2], ["gone.txt", 1]]);
  const back = takeDraft(draft, "merge:x:y", ["open.txt"]);
  assert.equal(mergedCount(back, "done.txt"), 2);
  assert.equal(mergedCount(back, "gone.txt"), 1);
  // Another step starts with none; a draft from before has none.
  assert.equal(takeDraft(draft, "merge:x:z", ["open.txt"]).merged.size, 0);
  assert.deepEqual(readDraft({ version: 1, step: "x", savedAt: 1, seen: [], files: [], old: [] })?.merged, []);
  assert.deepEqual(readDraft({ version: 1, step: "x", savedAt: 1, seen: [], files: [], old: [], merged: [["a", "two"], "b", ["c", 1]] })?.merged, [["c", 1]]);
}

// A file resolved since isn't kept: what was written is in the file now.
{
  const state = empty();
  state.choices.set("a.txt", ["left"]);
  state.stages.set("a.txt", "s");
  assert.equal(makeDraft("merge:x:y", state, [], 1), null);
}

// A draft of another step comes back only as text to compare, with when it was saved.
{
  const state = empty();
  state.choices.set("a.txt", ["left"]);
  state.stages.set("a.txt", "s");
  state.results.set("a.txt", "one\nTWO on feature\n");
  const draft = roundTrip(makeDraft("rebase:c1:h1", state, ["a.txt"], 5000));
  const back = takeDraft(draft, "rebase:c2:h2", ["a.txt"]);
  assert.equal(back.choices.size, 0);
  assert.deepEqual(back.old.get("a.txt"), { path: "a.txt", result: "one\nTWO on feature\n", savedAt: 5000 });
  // Kept on disk with the next save, until discarded or the file is resolved.
  const again = roundTrip(makeDraft("rebase:c2:h2", back, ["a.txt"], 6000));
  assert.deepEqual(again?.old, [{ path: "a.txt", result: "one\nTWO on feature\n", savedAt: 5000 }]);
  assert.equal(makeDraft("rebase:c2:h2", back, [], 6000), null);
}

// What a later version wrote, or a file that isn't a draft, is no draft; a broken entry is left out.
{
  assert.equal(readDraft(null), null);
  assert.equal(readDraft({ version: 2, step: "x", savedAt: 1, seen: [], files: [], old: [] }), null);
  const draft = readDraft({
    version: 1,
    step: "x",
    savedAt: 1,
    seen: [],
    files: [{ path: "a.txt", stages: "s", choices: ["sideways"], text: null, result: "" }, { path: "b.txt", stages: "s", choices: ["left"], text: null, result: "b" }],
    old: [],
  });
  assert.deepEqual(draft?.files.map((f) => f.path), ["b.txt"]);
}

// The result to compare is the file as it would be written: with its last line break or
// without, and an empty file when every line went.
{
  assert.equal(resultOf(file(), ["left"], null, sides), "one\nTWO on feature\n");
  assert.equal(resultOf(file({ openEnd: true }), ["right"], null, sides), "one\n2 on main");
  assert.equal(resultOf(file(), [null], null, sides), "one\n<<<<<<< feature\nTWO on feature\n=======\n2 on main\n>>>>>>> main\n");
  const gone = file({ chunks: [{ kind: "conflict", ours: ["x"], base: [], theirs: ["y"] }] });
  assert.equal(resultOf(gone, [{ lines: [] }], null, sides), "");
  assert.equal(resultOf(file(), ["left"], { text: "typed\r\n", start: "", stray: 0 }, sides), "typed\r\n");
}
