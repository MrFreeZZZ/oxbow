import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import type { CommitFileContext } from "../src/lib/changesMenu.ts";
import { commitFileActions, commitFileRequest } from "../src/lib/commitFiles.ts";
import { shortId, splitPath } from "../src/lib/format.ts";
import type { MenuEntry } from "../src/lib/Menu.svelte";

// The menu builder imports Svelte and Tauri. Load the real construction without them.
const source = fs.readFileSync(new URL("../src/lib/changesMenu.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "")
  .replace(/^export /gm, "");
const sha = "0123456789abcdef0123456789abcdef01234567";
const repo = "/repo";
const dialog: { to: string | null; options: { title?: string; defaultPath?: string } | null } = { to: "/tmp/app.patch", options: null };
const saved: unknown[][] = [];
const notes: unknown[][] = [];
const failures: unknown[] = [];
const nav = {
  fileMode: "changes",
  files: [] as unknown[],
  openFile(...args: unknown[]) { this.files.push(args); },
  openBrowse() {},
};
const api = {
  saveFilePatch: async (...args: unknown[]) => { saved.push(args); },
  revealFile: async () => {},
};
const confirm = {
  say(...args: unknown[]) { notes.push(args); },
  fail(err: unknown) { failures.push(err); },
};
const globals = {
  save: async (options: { title?: string; defaultPath?: string }) => { dialog.options = options; return dialog.to; },
  api, confirm, shortId, splitPath, commitFileActions, commitFileRequest, nav,
  ignoreChoices: () => [], ignoreRequest: () => ({}), stashFilesRequest: () => ({}), stashMessageFor: () => "",
  stashStagedRequest: () => ({}),
  navigator: { platform: "MacIntel" },
  menuIcons: new Proxy({}, { get: () => "" }),
  workingFileMenuItem: () => ({ kind: "item", label: "Open Working Copy", icon: "", run() {} }),
  NEWEST_SAVED: "local",
};
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
const commitFileMenu = Function(...Object.keys(globals), `${js}; return commitFileMenu;`)(...Object.values(globals)) as (
  scope: Pick<typeof globals, "api" | "confirm" | "nav">,
  repo: string,
  file: { path: string; status: string; oldPath?: string | null; submodule?: unknown },
  commit: string,
  ctx: CommitFileContext,
  canBlame: () => boolean,
) => { entries: MenuEntry[]; label: string };

const file = (status: string, more: { oldPath?: string | null; submodule?: unknown; path?: string } = {}) => ({ path: "src/app.py", status, ...more });
const ctx = { summary: "Edit the app", inHead: true, compare() {} };
function menu(entry = file("modified"), canBlame: () => boolean = () => false, context: CommitFileContext = ctx) {
  return commitFileMenu(globals, repo, entry, sha, context, canBlame);
}
function visibleLabels(entries: MenuEntry[]) {
  return entries.filter((entry) => entry.kind !== "item" || !entry.hidden)
    .map((entry) => entry.kind === "sep" ? "---" : entry.label);
}
function item(entries: MenuEntry[], label: string) {
  const found = entries.find((entry) => entry.kind === "item" && entry.label === label);
  assert.ok(found?.kind === "item", label);
  return found;
}
function reset() {
  dialog.to = "/tmp/app.patch";
  dialog.options = null;
  saved.length = 0;
  notes.length = 0;
  failures.length = 0;
  nav.fileMode = "changes";
  nav.files.length = 0;
  api.saveFilePatch = async (...args: unknown[]) => { saved.push(args); };
}

// Blame keeps its slot between File History and Browse when the caller reveals it.
{
  let allow = false;
  const entries = menu(file("modified"), () => allow).entries;
  const blame = item(entries, "Blame at This Commit");
  const order = [
    "app.py · modified in this commit",
    "Revert This File", "Take This Version…", "Compare with Working Copy", "Save File Patch…",
    "---",
    "File History", "Blame at This Commit", "Browse Files at This Commit",
    "---",
    "Open Working Copy", "Reveal in Finder", "Local History", "Copy Path/Reference",
  ];
  const at = order.indexOf(blame.label);
  assert.equal(entries[at], blame);
  assert.equal(blame.hidden, true);
  assert.deepEqual(visibleLabels(entries), order.filter((label) => label !== blame.label));
  allow = true;
  assert.equal(blame.hidden, false);
  assert.equal(entries[at], blame);
  assert.deepEqual(visibleLabels(entries), order);
  assert.equal(entries.at(-1)?.kind, "sub");
  blame.run();
  assert.equal(nav.fileMode, "blame");
  assert.deepEqual(nav.files, [[file().path, sha, { base: sha, at: sha }]]);
}

// The action group starts with the commands allowed by the commit's relation to HEAD.
for (const [inHead, actions] of [
  [false, ["Cherry-Pick This File"]],
  [null, ["Revert This File", "Cherry-Pick This File"]],
] as const) {
  const entries = visibleLabels(menu(file("modified"), undefined, { ...ctx, inHead, split() {} }).entries);
  assert.deepEqual(entries.slice(1, entries.indexOf("---")), [
    ...actions, "Take This Version…", "Compare with Working Copy", "Split into a New Commit…", "Save File Patch…",
  ]);
}

// A rename's patch names both paths. A copy, a modification and a deletion name only the file.
{
  for (const [entry, oldPath] of [
    [file("renamed", { oldPath: "lib/old.py" }), "lib/old.py"],
    [file("renamed"), null],
    [file("copied", { oldPath: "lib/old.py" }), null],
    [file("modified", { oldPath: "lib/old.py" }), null],
    [file("deleted"), null],
  ] as const) {
    reset();
    await item(menu(entry).entries, "Save File Patch…").run();
    assert.deepEqual(dialog.options, {
      title: "Save File Patch",
      defaultPath: `${shortId(sha)}-app.py.patch`,
      filters: [{ name: "Patch", extensions: ["patch"] }],
    });
    assert.deepEqual(saved, [[repo, sha, entry.path, oldPath, dialog.to]]);
    assert.deepEqual(notes, [[`Saved a patch for app.py.`, "done"]]);
  }
  reset();
  dialog.to = null;
  await item(menu().entries, "Save File Patch…").run();
  assert.deepEqual(saved, []);
  assert.deepEqual(notes, []);
  reset();
  api.saveFilePatch = async () => { throw new Error("disk full"); };
  await item(menu(file("modified", { path: "dir/text[1].txt" })).entries, "Save File Patch…").run();
  assert.equal(dialog.options?.defaultPath, `${shortId(sha)}-text[1].txt.patch`);
  assert.deepEqual(saved, []);
  assert.deepEqual(notes, []);
  assert.equal(failures.length, 1);
}

// Deleted files retain patch export. A submodule only has an action group when Split is offered.
for (const split of [undefined, () => {}]) {
  const context = { ...ctx, split };
  const splitLabels = split ? ["Split into a New Commit…"] : [];
  assert.deepEqual(visibleLabels(menu(file("deleted"), undefined, context).entries), [
    "app.py · deleted in this commit",
    "Revert This File", ...splitLabels, "Save File Patch…",
    "---",
    "File History",
    "---",
    "Open Working Copy", "Local History", "Copy Path/Reference",
  ]);
  const submodule = file("modified", { path: "vendor/module", submodule: { moved: true } });
  assert.deepEqual(visibleLabels(menu(submodule, undefined, context).entries), [
    "module · modified in this commit",
    ...(split ? [...splitLabels, "---"] : []),
    "File History",
    "---",
    "Reveal in Finder", "Local History", "Copy Path/Reference",
  ]);
}

// Deletions and submodules never get that allowance: the panel returns before the binary check.
// A text file in the same menu and repository reveals Blame; a binary file, a closed menu, another
// commit or another repository does not.
{
  const panel = fs.readFileSync(new URL("../src/lib/CommitPanel.svelte", import.meta.url), "utf8");
  const openFileMenu = panel.match(/function openFileMenu\(event: MouseEvent, file: FileChange\) \{[\s\S]*?\n  \}/);
  assert.ok(openFileMenu, "CommitPanel openFileMenu");
  assert.match(openFileMenu[0], /let canBlame = \$state\(false\)/);
  assert.match(openFileMenu[0], /commitFileMenu\(repo, file, commit, ctx, \(\) => canBlame\)/);
  assert.match(openFileMenu[0], /if \(file\.status === "deleted" \|\| file\.submodule\) return;/);
  assert.match(openFileMenu[0], /api\.fileIsBinaryAt\(commit, file\.path\)\.then\(\(binary\) => \{\n\s*if \(!binary && menu === opened && row\.id === commit && nav\.repo === repo\)\n\s*canBlame = true;\n\s*\}, \(\) => \{\}\)/);
}

// A hidden item stays in the model at its index and is not a menu button, so focus does not move.
{
  const view = fs.readFileSync(new URL("../src/lib/Menu.svelte", import.meta.url), "utf8");
  const list = view.slice(view.indexOf("{#each entries as entry, i (i)}"), view.indexOf("{#if sub}"));
  const shown = list.indexOf("{:else if !entry.hidden}");
  const index = list.indexOf("data-index={i}");
  assert.ok(shown > 0 && index > shown);
}
