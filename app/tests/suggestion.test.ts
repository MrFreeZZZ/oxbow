import { applyRequest, bodyParts, hunkText, insertBlock, suggestedRange, suggestionBlock, threadText } from "../src/lib/suggestion.ts";
import type { DiffLine, Hunk } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

test("a comment splits into its words and its suggestions", () => {
  assert.deepEqual(bodyParts("Use the constant:\r\n```suggestion\r\nconst LIMIT = 10;\r\n```\r\nThanks!"), [
    { kind: "text", text: "Use the constant:" },
    { kind: "suggestion", lines: ["const LIMIT = 10;"], above: 0, below: 0 },
    { kind: "text", text: "Thanks!" },
  ]);
  assert.deepEqual(bodyParts("Just words"), [{ kind: "text", text: "Just words" }]);
  assert.deepEqual(bodyParts("Just words\n```js\nx\n```"), [{ kind: "text", text: "Just words\n```js\nx\n```" }]);
  assert.deepEqual(bodyParts("~~~suggestion\nx\n~~~"), [{ kind: "suggestion", lines: ["x"], above: 0, below: 0 }]);
});

test("an empty suggestion takes the lines out, and GitLab's says how far it reaches", () => {
  assert.deepEqual(bodyParts("```suggestion\n```"), [{ kind: "suggestion", lines: [], above: 0, below: 0 }]);
  assert.deepEqual(bodyParts("```suggestion:-2+1\na\nb\n```"), [{ kind: "suggestion", lines: ["a", "b"], above: 2, below: 1 }]);
});

test("a longer fence keeps backticks inside, and an open one runs to the end", () => {
  assert.deepEqual(bodyParts("````suggestion\n```js\n```\n````"), [{ kind: "suggestion", lines: ["```js", "```"], above: 0, below: 0 }]);
  assert.deepEqual(bodyParts("```suggestion\nlast"), [{ kind: "suggestion", lines: ["last"], above: 0, below: 0 }]);
});

test("a suggestion covers its comment's lines on GitHub and its reach on GitLab", () => {
  const [one] = bodyParts("```suggestion\nx\n```");
  const [reach] = bodyParts("```suggestion:-2+1\nx\n```");
  assert.equal(one.kind, "suggestion");
  assert.equal(reach.kind, "suggestion");
  if (one.kind !== "suggestion" || reach.kind !== "suggestion") return;
  assert.deepEqual(suggestedRange(one, { line: 14, startLine: 12 }, false), { start: 12, end: 14 });
  assert.deepEqual(suggestedRange(one, { line: 14, startLine: null }, false), { start: 14, end: 14 });
  assert.deepEqual(suggestedRange(reach, { line: 14 }, true), { start: 12, end: 15 });
  assert.deepEqual(suggestedRange(one, { line: 14, startLine: 12 }, true), { start: 14, end: 14 });
  assert.equal(suggestedRange(one, { line: null }, false), null);
});

test("a suggestion is written in a fence its lines can't close", () => {
  assert.equal(suggestionBlock(["a", "b"], false, 2), "```suggestion\na\nb\n```");
  assert.equal(suggestionBlock(["a", "b"], true, 2), "```suggestion:-1+0\na\nb\n```");
  assert.equal(suggestionBlock(["```js"], false, 1), "````suggestion\n```js\n````");
});

test("the block goes on lines of its own", () => {
  assert.deepEqual(insertBlock("", 0, "B"), { value: "B\n", start: 0 });
  assert.deepEqual(insertBlock("Try this:", 9, "B"), { value: "Try this:\nB\n", start: 10 });
  assert.deepEqual(insertBlock("a\nz", 2, "B"), { value: "a\nB\nz", start: 2 });
});

const line = (kind: DiffLine["kind"], oldLine: number | null, newLine: number | null, text: string): DiffLine => ({ kind, oldLine, newLine, text, words: null });

test("the lines a suggestion starts from come from the diff or the thread", () => {
  const hunk: Hunk = {
    header: "",
    oldStart: 1,
    oldLines: 2,
    newStart: 1,
    newLines: 3,
    lines: [line("context", 1, 1, "a"), line("removed", 2, null, "old"), line("added", null, 2, "b"), line("added", null, 3, "c")],
  };
  assert.deepEqual(hunkText(hunk, 1, 3), ["a", "b", "c"]);
  assert.equal(hunkText(hunk, 3, 4), null);
  assert.deepEqual(
    threadText(
      [
        { number: null, text: "old", mark: false },
        { number: 7, text: "x", mark: true },
      ],
      7,
      7,
    ),
    ["x"],
  );
});

test("Apply Suggestion puts the lines on disk, uncommitted", () => {
  const request = applyRequest("src/limits.ts", "abc1234", { start: 12, end: 14 }, ["const LIMIT = 10;"], "maria");
  assert.equal(request.title, "Apply maria’s suggestion to limits.ts?");
  assert.deepEqual(request.action, { kind: "applySuggestion", commit: "abc1234", path: "src/limits.ts", start: 12, end: 14, lines: ["const LIMIT = 10;"] });
  assert.ok(request.body.includes("lines 12–14"));
  assert.ok(applyRequest("a", "c", { start: 3, end: 3 }, [], "me").body.includes("line 3"));
  // No lines take them out; one empty line leaves a blank one: the two stay apart.
  const lines = (l: string[]) => (applyRequest("a", "c", { start: 3, end: 3 }, l, "me").action as { lines: string[] }).lines;
  assert.deepEqual(lines([]), []);
  assert.deepEqual(lines([""]), [""]);
});
