import { goneScreen, messageOf } from "../src/lib/gone.ts";
import type { Gone } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const shown = (path: string) => path.replace("/Users/alex", "~");
const none = { message: null, conflicts: false };
const worktree = (over: Partial<Gone> = {}): Gone => ({
  path: "/Users/alex/src/acme-login",
  name: "acme-login",
  why: "missing",
  disk: null,
  found: null,
  worktree: { main: "/Users/alex/src/acme", mainBranch: "main", branch: "fix/login", head: null, locked: null },
  conflictDraft: false,
  ...over,
});

test("a deleted worktree offers its main folder first and says its branch is safe", () => {
  const screen = goneScreen(worktree(), none, shown, true);
  assert.equal(screen.title, "acme-login isn’t there any more");
  assert.equal(screen.primary, "main");
  assert.equal(screen.main, "/Users/alex/src/acme");
  assert.deepEqual(
    screen.facts.map((f) => f.tone),
    ["safe", "warn", "note"],
  );
  assert.match(screen.facts[0].text, /fix\/login and its commits are safe in acme/);
  assert.match(screen.facts[2].text, /Prune under Worktrees in acme/);
});

test("a locked worktree is not offered Prune", () => {
  const screen = goneScreen(worktree({ worktree: { main: "/Users/alex/src/acme", mainBranch: "main", branch: "fix/login", head: null, locked: "on the USB drive" } }), none, shown, true);
  assert.match(screen.facts.at(-1)!.text, /locked \(“on the USB drive”\)/);
});

test("drafts stay with the folder and don't go to the main folder's branch", () => {
  const screen = goneScreen(worktree({ conflictDraft: true }), { message: "Fix login timeout", conflicts: true }, shown, true);
  const kept = screen.facts.find((f) => f.text.startsWith("Oxbow keeps"))!;
  assert.match(kept.text, /commit message “Fix login timeout” and the choices on the Conflicts screen/);
  assert.match(kept.text, /Open Main Folder doesn’t move them to main\./);
});

test("each reason has its own way out", () => {
  assert.equal(goneScreen(worktree({ why: "disk", disk: "Work SSD" }), none, shown, true).primary, "retry");
  assert.match(goneScreen(worktree({ why: "disk", disk: "Work SSD" }), none, shown, true).reason, /“Work SSD”/);
  const moved = goneScreen(worktree({ why: "moved", found: "/Users/alex/src/login-old" }), none, shown, true);
  assert.equal(moved.primary, "find");
  assert.match(moved.reason, /~\/src\/login-old/);
  assert.equal(goneScreen(worktree({ why: "trash" }), none, shown, true).primary, "retry");
  assert.match(goneScreen(worktree({ why: "noAccess" }), none, shown, true).reason, /System Settings/);
  assert.doesNotMatch(goneScreen(worktree({ why: "noAccess" }), none, shown, false).reason, /System Settings/);
  // A main folder that is gone has no main folder to offer.
  const main = goneScreen(worktree({ worktree: null }), none, shown, true);
  assert.equal(main.primary, "find");
  assert.equal(main.main, null);
  assert.match(main.facts[0].text, /commits that weren’t pushed/);
});

test("the commit message draft is named by its summary", () => {
  assert.equal(messageOf({ summary: " Fix login ", description: "" }), "Fix login");
  assert.equal(messageOf({ summary: "", description: "First line\nmore" }), "First line");
  assert.equal(messageOf({ summary: "", description: "" }), null);
  assert.equal(messageOf(null), null);
  assert.equal(messageOf({ summary: "x".repeat(60) })!.length, 48);
});
