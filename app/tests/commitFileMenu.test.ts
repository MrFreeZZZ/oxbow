import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
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
  menuIcons: new Proxy({}, { get: () => "" }),
  workingFileMenuItem: () => ({ kind: "item", label: "Open", icon: "", run() {} }),
  NEWEST_SAVED: "local",
};
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
const commitFileMenu = Function(...Object.keys(globals), `${js}; return commitFileMenu;`)(...Object.values(globals)) as (
  repo: string,
  file: { path: string; status: string; oldPath?: string | null; submodule?: unknown },
  commit: string,
  ctx: { summary: string | null; inHead: boolean | null; compare: () => void },
  canBlame: () => boolean,
) => { entries: MenuEntry[]; label: string };

const file = (status: string, more: { oldPath?: string | null; submodule?: unknown; path?: string } = {}) => ({ path: "src/app.py", status, ...more });
const ctx = { summary: "Edit the app", inHead: true, compare() {} };
function menu(entry = file("modified"), canBlame: () => boolean = () => false) {
  return commitFileMenu(repo, entry, sha, ctx, canBlame);
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

// Blame keeps its place between File History and Local History, hidden until the caller allows it.
{
  let allow = false;
  const entries = menu(file("modified"), () => allow).entries;
  const blame = item(entries, "Blame at This Commit");
  const at = entries.indexOf(blame);
  assert.ok(at > entries.findIndex((entry) => entry.kind === "item" && entry.label === "File History"));
  assert.ok(at < entries.findIndex((entry) => entry.kind === "item" && entry.label === "Local History"));
  assert.equal(blame.hidden, true);
  allow = true;
  assert.equal(blame.hidden, false);
  assert.equal(entries.indexOf(blame), at);
  blame.run();
  assert.equal(nav.fileMode, "blame");
  assert.deepEqual(nav.files, [[file().path, sha, { base: sha, at: sha }]]);
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

// A submodule is not a file patch. Blame stays a normal item: the panel, not this menu, refuses to allow it.
{
  const entries = menu(file("modified", { submodule: { moved: true } }), () => true).entries;
  assert.equal(entries.some((entry) => entry.kind === "item" && entry.label === "Save File Patch…"), false);
  assert.equal(item(entries, "Blame at This Commit").hidden, false);
  assert.equal(entries.some((entry) => entry.kind === "item" && entry.label === "Browse Files at This Commit"), false);
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
