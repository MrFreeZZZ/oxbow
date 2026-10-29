import { applyFix, drawnLines, LINE_BLOCK, lineStart, mergedCheck, messageParts, paintLine, problemsByLine, rowSub, status } from "../src/lib/repoFiles.ts";
import type { FileCheck, FileProblem, RepoFile } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const problem = (line: number, from: number, to: number, level: "error" | "warn"): FileProblem => ({ line, from, to, level, message: "x", fix: null });

test("a fix replaces its line or takes it out, keeping a \\r", () => {
  const text = "a\r\nbuild\\out\r\nc";
  assert.equal(applyFix(text, { kind: "replace", label: "", line: 2, text: "build/out" }), "a\r\nbuild/out\r\nc");
  assert.equal(applyFix("a\nb\nc", { kind: "replace", label: "", line: 2, text: null }), "a\nc");
  assert.equal(applyFix("a", { kind: "replace", label: "", line: 5, text: "x" }), "a");
});

test("messages split into words and code", () => {
  assert.deepEqual(messageParts("Use `/` between folders."), [
    { text: "Use ", code: false },
    { text: "/", code: true },
    { text: " between folders.", code: false },
  ]);
});

test("a line is cut where its underlines start and end, an error over a warning", () => {
  const pieces = paintLine("gitignore", "!logs/keep.txt", [problem(1, 0, 14, "warn"), problem(1, 1, 5, "error")]);
  assert.deepEqual(
    pieces.map((p) => [p.text, p.level]),
    [
      ["!", "warn"],
      ["logs", "error"],
      ["/keep.txt", "warn"],
    ],
  );
  assert.equal(paintLine("gitignore", "# build", [])[0].comment, true);
  assert.equal(paintLine("editorconfig", "; old", [])[0].comment, true);
  assert.equal(paintLine("gitignore", "; not a comment", [])[0].comment, false);
});

test("lines start where they start", () => {
  assert.equal(lineStart("ab\ncd\nef", 1), 0);
  assert.equal(lineStart("ab\ncd\nef", 3), 6);
});

test("the row says what the file holds, or its first error", () => {
  const file: RepoFile = { kind: "gitignore", path: ".gitignore", exists: true, rules: 18, uncommitted: false, problem: null };
  assert.equal(rowSub(file, false), "Files git leaves out · 18 rules");
  assert.equal(rowSub({ ...file, kind: "blameIgnoreRevs", rules: 1 }, false), "Commits Blame skips, such as reformatting · 1 commit");
  assert.equal(rowSub({ ...file, kind: "codeowners", exists: false, rules: 0 }, true), "Who GitLab asks to review each folder");
  assert.equal(rowSub({ ...file, problem: { ...problem(4, 0, 1, "error"), message: "`eol=crfl` is not `lf`" } }, false), "Line 4: eol=crfl is not lf");
});

test("the bar says what stops saving", () => {
  assert.deepEqual(status([problem(1, 0, 1, "error"), problem(2, 0, 1, "warn")], true, true, false), { text: "Fix 1 error to save. Warnings don’t stop saving.", error: true });
  assert.equal(status([], true, true, false).text, "Not saved yet");
  assert.equal(status([], false, true, true).text, "Saved, not committed yet: commit it in Changes");
  assert.equal(status([], true, false, false).text, "Not created yet");
});

// A long file draws whole blocks of lines around what is on screen, a screen more each side.
{
  // 20 px lines, 400 px on screen, scrolled to line 1,000.
  const drawn = drawnLines(20_000, 400, 20, 100_000);
  assert.ok(drawn.from <= 980 && drawn.to >= 1040, JSON.stringify(drawn));
  assert.equal(drawn.from % LINE_BLOCK, 0);
  assert.ok(drawn.to - drawn.from <= 4 * LINE_BLOCK);
  assert.deepEqual(drawnLines(0, 400, 20, 10), { from: 0, to: 10 });
  // Not measured yet: every line.
  assert.deepEqual(drawnLines(0, 0, 0, 500), { from: 0, to: 500 });
}

// The problems of a line are looked up by its number.
{
  const p = (line: number, message: string) => ({ line, from: 0, to: 1, level: "warn", message }) as FileProblem;
  const map = problemsByLine([p(2, "a"), p(5, "b"), p(2, "c")]);
  assert.deepEqual(map.get(2)?.map((x) => x.message), ["a", "c"]);
  assert.equal(map.get(3), undefined);
}

// Checked by the text alone while the working copy is read, the notes shown stay.
{
  const shown = { problems: [], notes: [{ line: 1, text: "3 files", level: null }] } as FileCheck;
  assert.deepEqual(mergedCheck(shown, { problems: [], notes: [], pending: true }).notes, shown.notes);
  assert.deepEqual(mergedCheck(shown, { problems: [], notes: [] }).notes, []);
}
