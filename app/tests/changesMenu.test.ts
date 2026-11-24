import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { discardPartRequest, shareOf } from "../src/lib/changelists.ts";
import type { FileMenuActions } from "../src/lib/changesMenu.ts";
import type { Request } from "../src/lib/confirm.svelte.ts";
import { splitPath } from "../src/lib/format.ts";
import type { MenuEntry } from "../src/lib/Menu.svelte";
import { stashStagedRequest } from "../src/lib/stashLines.ts";
import type { Changelists, FileChange } from "../src/lib/types.ts";

// Run the real builders while substituting the Svelte and Tauri dependencies.
function load<T>(module: string, exports: string, globals: Record<string, unknown>): T {
  const source = fs.readFileSync(new URL(`../src/lib/${module}.ts`, import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "")
    .replace(/^export /gm, "");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  return Function(...Object.keys(globals), `${js}; return { ${exports} };`)(...Object.values(globals));
}

type ChangesRequests = Pick<typeof import("../src/lib/changes.ts"),
  "discardRequest" | "ignoreChoices" | "ignoreRequest" | "stashFilesRequest" | "stashMessageFor">;
const requests = load<ChangesRequests>(
  "changes", "discardRequest, ignoreChoices, ignoreRequest, stashFilesRequest, stashMessageFor", { splitPath },
);
const scoped = load<Pick<typeof import("../src/lib/changesMenu.ts"), "fileMenu" | "filesMenu">>("changesMenu", "fileMenu, filesMenu", {
  ...requests, splitPath, stashStagedRequest, navigator: { platform: "MacIntel" },
  menuIcons: new Proxy({}, { get: () => "" }),
  workingFileMenuItem: () => ({ kind: "item", label: "Open Working Copy", icon: "", run() {} }),
});

const scope = {} as Parameters<typeof scoped.fileMenu>[0];
const fileMenu = scoped.fileMenu.bind(null, scope);
const filesMenu = scoped.filesMenu.bind(null, scope);

const file = (status: FileChange["status"], path = "src/app.py"): FileChange => ({
  path, status, oldPath: null, additions: 0, deletions: 0, binary: false,
});
const modified = file("modified");
const fresh = file("untracked", "src/new.py");
const second = file("untracked", "src/other.py");
const staged = file("added", "src/staged.py");
const move: MenuEntry = { kind: "sub", label: "Move to Changelist", icon: "", entries: [] };
const actions: FileMenuActions = {
  stage() {}, unstage() {}, discard() {}, act() {}, actOnPicked() {},
  extra: [move],
  stagedCount: () => 3,
  stashCandidates: () => [
    { path: modified.path, untracked: false, staged: true, unstaged: true },
    { path: fresh.path, untracked: true, staged: false, unstaged: true },
    { path: staged.path, untracked: false, staged: true, unstaged: false },
  ],
};
const repo = "/repo";
const labels = (entries: MenuEntry[]) => entries.map((entry) => entry.kind === "sep" ? "---" : entry.label);

function dangerItem(entries: MenuEntry[], label?: string) {
  const dangers = entries.filter((entry) => entry.kind === "item" && entry.danger);
  assert.deepEqual(labels(dangers), label ? [label] : []);
  if (label) {
    const last = entries.at(-1);
    assert.equal(last, dangers[0]);
    assert.ok(last?.kind === "item");
    return last;
  }
}

test("single-file menus preserve order, groups and the final danger action", () => {
  const stage = ["Stage File", move.label, "Stash This File…"];
  const unstage = ["Unstage File", move.label, "Stash This File…", "Stash All Staged Changes…"];
  const ignore = "Add to .gitignore";
  const open = "Open Working Copy";
  const reveal = "Reveal in Finder";
  const copy = "Copy Path/Reference";
  const history = ["File History", "Local History"];
  const discard = "Discard Changes…";
  const del = "Delete File…";
  for (const [entry, side, header, operations, tools, versions, danger] of [
    [modified, "unstaged", "app.py · modified", [...stage, ignore], [open, reveal, copy], history, discard],
    [fresh, "untracked", "new.py · untracked", [...stage, ignore], [open, reveal, copy], ["Local History"], del],
    [modified, "staged", "app.py · modified, staged", [...unstage, ignore], [open, reveal, copy], history, undefined],
    [file("conflicted"), "conflicted", "app.py · conflicted", [], [open, reveal, copy], history, undefined],
    [file("deleted"), "unstaged", "app.py · deleted", stage, [open, copy], history, discard],
    [staged, "staged", "staged.py · new file, staged", [...unstage, ignore], [open, reveal, copy], ["Local History"], undefined],
    [{ ...modified, submodule: { moved: true, changed: false } }, "unstaged", "app.py · modified", [...stage, ignore], [reveal, copy], history, discard],
  ] as const) {
    const calls: FileChange[][] = [];
    const { entries } = fileMenu(repo, entry, side, {
      ...actions, extra: side === "conflicted" ? [] : actions.extra,
      discard: (files) => calls.push(files),
    });
    assert.deepEqual(labels(entries), [header, ...operations, "---", ...tools, "---", ...versions, ...(danger ? ["---", danger] : [])]);
    dangerItem(entries, danger)?.run();
    assert.deepEqual(calls, danger ? [[entry]] : []);
  }
});

test("multi-file menus group staged, new and mixed selections", () => {
  for (const [files, side, where, danger] of [
    [[fresh, second], "untracked", undefined, "Delete 2 Files…"],
    [[fresh, modified], "unstaged", "in Login fix", "Discard 2 Files…"],
    [[staged, modified], "staged", undefined, undefined],
  ] as const) {
    const calls: string[] = [];
    const { entries } = filesMenu(repo, [...files], side, { ...actions, where, actOnPicked: (how) => calls.push(how ?? "move") });
    assert.deepEqual(labels(entries), [
      `2 selected files${where ? ` ${where}` : ""}`,
      side === "staged" ? "Unstage 2 Files" : "Stage 2 Files",
      move.label, "Stash These Files…", ...(side === "staged" ? ["Stash All Staged Changes…"] : []),
      "---", "Copy Path/Reference", ...(danger ? ["---", danger] : []),
    ]);
    dangerItem(entries, danger)?.run();
    assert.deepEqual(calls, danger ? ["discard"] : []);
  }
});

test("Delete and Discard confirmations describe the selected files and preserve Undo", () => {
  for (const [files, title, button, done, body] of [
    [[fresh], "Delete new.py?", "Delete", "Deleted new.py.", ["Deletes ", { code: fresh.path }, ". Git does not track ", "it", " yet."]],
    [[fresh, second], "Delete 2 files?", "Delete", "Deleted 2 files.", ["Deletes ", "2 files", ". Git does not track ", "them", " yet."]],
    [[modified, fresh], "Discard changes in 2 files?", "Discard", "Discarded changes in 2 files.", ["Discards unstaged changes in ", { code: modified.path }, " and deletes ", { code: fresh.path }, ", which Git does not track yet."]],
  ] satisfies [FileChange[], string, string, string, Request["body"]][]) {
    const discard = requests.discardRequest(files);
    assert.deepEqual([discard.title, discard.button, discard.done, discard.body], [title, button, done, body]);
    assert.equal(discard.danger, true);
    assert.match(discard.note ?? "", /Undo brings it back/);
    assert.deepEqual(discard.action, { kind: "discard", paths: files.map((f) => f.path) });
  }

  const list = "login";
  const lists: Changelists = {
    lists: [{ id: list, name: "Login fix", paths: [modified.path, fresh.path] }],
    active: list,
    split: [{ path: modified.path, parts: [
      { list, runs: ["first"], additions: 1, deletions: 0 },
      { list: null, runs: ["second"], additions: 1, deletions: 0 },
    ] }],
  };
  assert.deepEqual(shareOf(lists, modified.path, list)?.others, [null]);
  const discard = discardPartRequest([modified, fresh], list, lists);
  assert.equal(discard.title, "Discard the changes of Login fix in 2 files?");
  assert.equal(discard.button, "Discard");
  assert.equal(discard.danger, true);
  assert.deepEqual(discard.body, [
    "Discards the unstaged changes of ", { quote: "Login fix" }, " in ", { code: modified.path },
    ". Changes of other lists in the same files stay as they are.", " Deletes ", { code: fresh.path }, ", which Git does not track yet.",
  ]);
  assert.deepEqual(discard.action, { kind: "discardChangelist", list, paths: [modified.path, fresh.path] });
});

test("stash menu actions keep selected-file and all-staged scopes distinct", () => {
  const sent: Request[] = [];
  const a = { ...actions, act: (request: Request) => sent.push(request) };
  const single = fileMenu(repo, modified, "staged", a).entries;
  const several = filesMenu(repo, [modified, fresh], "unstaged", a).entries;
  for (const [entries, label] of [[single, "Stash This File…"], [several, "Stash These Files…"], [single, "Stash All Staged Changes…"]] as const) {
    const item = entries.find((entry) => entry.kind === "item" && entry.label === label);
    assert.ok(item?.kind === "item", label);
    item.run();
  }
  const [one, many, allStaged] = sent;
  assert.deepEqual(one.action, { kind: "stashPush", message: "WIP: app", untracked: false, paths: [modified.path] });
  assert.deepEqual(many.action, { kind: "stashPush", message: "WIP: app", untracked: true, paths: [modified.path, fresh.path] });
  for (const [request, picked] of [[one, [modified.path]], [many, [modified.path, fresh.path]]] as const)
    assert.deepEqual(request.options?.filter((option) => option.on).map((option) => option.label), picked);
  assert.equal(one.options?.[0].sub, "Staged and unstaged changes");
  assert.equal(allStaged.title, "Stash 3 staged files?");
  assert.deepEqual(allStaged.action, { kind: "stashPush", message: null, untracked: false, staged: true });
});

test("tracked Ignore names and confirms what Git will stop tracking", () => {
  const sent: Request[] = [];
  const a = { ...actions, act: (request: Request) => sent.push(request) };
  for (const [entry, side, expected] of [
    [modified, "unstaged", ["Stop Tracking and Ignore “app.py”", "Stop Tracking and Ignore Folder “src/”"]],
    [fresh, "untracked", ["Ignore “new.py”", "Ignore All *.py Files", "Ignore Folder “src/”"]],
  ] as const) {
    const ignore = fileMenu(repo, entry, side, a).entries.find((item) => item.kind === "sub" && item.label === "Add to .gitignore");
    assert.ok(ignore?.kind === "sub");
    assert.deepEqual(ignore.entries.map((item) => item.label), expected);
    ignore.entries[0].run();
    ignore.entries.at(-1)?.run();
  }
  assert.deepEqual(sent.map(({ always, action }) => ({ always, action })), [
    { always: true, action: { kind: "ignore", pattern: "/src/app.py", untrack: modified.path } },
    { always: true, action: { kind: "ignore", pattern: "/src/", untrack: "src" } },
    { always: false, action: { kind: "ignore", pattern: "/src/new.py", untrack: undefined } },
    { always: false, action: { kind: "ignore", pattern: "/src/", untrack: undefined } },
  ]);
  const folderConfirmation = sent[1].body.filter((part) => typeof part === "string").join("");
  assert.match(folderConfirmation, /All tracked files in this folder stay on your disk\./);
  assert.match(folderConfirmation, /Git stages their removal from the repository for the next commit\./);
});
