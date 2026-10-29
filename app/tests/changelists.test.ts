import { groupFiles, hunkLists, runsOfLines, idOf, keyOf, listName, nameProblem, nameRequest, ownerOf, runList, shareOf, stagePartRequest, discardPartRequest } from "../src/lib/changelists.ts";
import { sameShown } from "../src/lib/changesView.ts";
import type { Changelists, FileChange, Hunk } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const file = (path: string, status: FileChange["status"] = "modified", additions = 3, deletions = 1) => ({ path, status, additions, deletions }) as FileChange;
const lists: Changelists = {
  lists: [
    { id: "a", name: "Login fix", paths: ["login.ts", "client.ts", "token.ts"] },
    { id: "b", name: "Logging", paths: ["log.ts", "client.ts"] },
    { id: "c", name: "Empty", paths: [] },
  ],
  active: "a",
  split: [
    {
      path: "client.ts",
      parts: [
        { list: "a", runs: ["k1"], additions: 2, deletions: 1 },
        { list: "b", runs: ["k2"], additions: 1, deletions: 1 },
        { list: null, runs: ["k3", "k4"], additions: 0, deletions: 2 },
      ],
    },
  ],
};
const unstaged = [file("main.ts"), file("login.ts"), file("client.ts"), file("token.ts", "untracked"), file("log.ts"), file("new.ts", "untracked")];

// Keys of the panel's lists and of changelists.
assert.equal(keyOf("a"), "cl:a");
assert.equal(idOf("cl:a"), "a");
assert.equal(idOf("unstaged"), null);

// Files sort into Unstaged, Untracked and each list; a shared file is in each that holds a part,
// with that part's lines.
const grouped = groupFiles(unstaged, lists);
assert.deepEqual(grouped.tracked.map((f) => [f.path, f.additions, f.deletions]), [["main.ts", 3, 1], ["client.ts", 0, 2]]);
assert.deepEqual(grouped.untracked.map((f) => f.path), ["new.ts"]);
assert.deepEqual(grouped.lists.map((g) => g.files.map((f) => [f.path, f.additions])), [[["login.ts", 3], ["client.ts", 2], ["token.ts", 3]], [["log.ts", 3], ["client.ts", 1]], []]);
// A list's file with no unstaged changes left isn't listed; without lists, nothing moves.
assert.deepEqual(groupFiles([file("main.ts")], lists).lists[0].files, []);
assert.deepEqual(groupFiles(unstaged, null).tracked.map((f) => f.path), ["main.ts", "login.ts", "client.ts", "log.ts"]);

// Where a run is, and the lists of a hunk.
assert.equal(runList(lists, "client.ts", "k2"), "b");
assert.equal(runList(lists, "client.ts", "k3"), null);
assert.equal(runList(lists, "login.ts", "any"), "a");
assert.equal(runList(lists, "main.ts", "any"), null);
const hunk = (runs: string[]) => ({ header: "@@", oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: [], runs }) as Hunk;
assert.deepEqual(hunkLists(lists, "client.ts", hunk(["k1", "k3", "k4", "k2"])), ["a", null, "b"]);
assert.deepEqual(hunkLists(lists, "login.ts", hunk([])), ["a"]);

// The runs of picked lines: changed lines between two unchanged ones are one run.
const lined = {
  header: "@@ -1,6 +1,7 @@",
  runs: ["r1", "r2"],
  lines: [
    { kind: "context", text: "a" },
    { kind: "removed", text: "b" },
    { kind: "added", text: "B" },
    { kind: "context", text: "c" },
    { kind: "added", text: "d" },
    { kind: "added", text: "e" },
    { kind: "context", text: "f" },
  ],
} as unknown as Hunk;
assert.deepEqual(runsOfLines(lined, [2]), ["r1"]);
assert.deepEqual(runsOfLines(lined, [5, 1]), ["r1", "r2"]);
assert.deepEqual(runsOfLines(lined, [0, 3]), []);
assert.deepEqual(runsOfLines({ ...lined, runs: undefined } as unknown as Hunk, [1]), []);
assert.equal(ownerOf(lists, "client.ts"), null);
assert.equal(ownerOf(lists, "log.ts"), "b");
assert.equal(ownerOf(null, "log.ts"), null);

// What a list holds of a shared file.
assert.deepEqual(shareOf(lists, "client.ts", "a"), { runs: 1, total: 4, others: ["b", null] });
assert.deepEqual(shareOf(lists, "client.ts", null), { runs: 2, total: 4, others: ["a", "b"] });
assert.equal(shareOf(lists, "login.ts", "a"), null);
assert.equal(listName(lists, null), "Unstaged");
assert.equal(listName(lists, "b"), "Logging");

// Names: trimmed, not taken, not one of the panel's own lists.
assert.equal(nameProblem("  Refactor ", lists.lists), null);
assert.match(nameProblem("login FIX", lists.lists) ?? "", /already/);
assert.equal(nameProblem("login FIX", lists.lists, "a"), null);
assert.match(nameProblem("Staged", lists.lists) ?? "", /lists of Changes/);
assert.match(nameProblem(" ", lists.lists) ?? "", /needs a name/);

// The sheet asking for a name says why it can't be used, and keeps it trimmed.
const saved: string[] = [];
const ask = nameRequest(lists.lists, null, "Logging", async (name) => (saved.push(name), "ok"), "src/log.ts");
assert.equal(ask.invalid, "There is a changelist called “Logging” already.");
assert.equal(ask.fields?.[0].error, ask.invalid);
const next = ask.fields![0].text!.edit(" Refactor ");
assert.equal(next.invalid, null);
assert.equal(next.button, "Create");
await next.local!.run();
assert.deepEqual(saved, ["Refactor"]);
// An empty name greys the button but says nothing yet.
const empty = nameRequest(lists.lists, null, "", async () => "");
assert.ok(empty.invalid);
assert.equal(empty.fields?.[0].error, undefined);
assert.equal(nameRequest(lists.lists, lists.lists[0], "Login", async () => "").title, "Rename “Login fix”?");

// Staging and discarding a list's part send the list with the files.
const stage = stagePartRequest([file("client.ts")], "b", lists, { code: "main" });
assert.deepEqual(stage.action, { kind: "stageChangelist", list: "b", paths: ["client.ts"] });
assert.equal(stage.past, "Staged client.ts of Logging");
const discard = discardPartRequest([file("a.ts"), file("b.ts")], null, lists);
assert.deepEqual(discard.action, { kind: "discardChangelist", list: null, paths: ["a.ts", "b.ts"] });
assert.equal(discard.title, "Discard the changes of Unstaged in 2 files?");
assert.ok(discard.danger);

// What the diff shows names its list.
assert.ok(sameShown({ kind: "file", path: "a", side: "unstaged" }, { kind: "file", path: "a", side: "unstaged", list: null }));
assert.ok(!sameShown({ kind: "file", path: "a", side: "unstaged", list: "a" }, { kind: "file", path: "a", side: "unstaged" }));
assert.ok(!sameShown({ kind: "all", side: "unstaged", list: "a" }, { kind: "all", side: "unstaged", list: "b" }));
