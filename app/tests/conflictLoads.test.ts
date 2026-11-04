import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { hasWork, makeDraft, readDraft, takeDraft, type DraftState } from "../src/lib/drafts.ts";
import { workingText } from "../src/lib/conflicts.ts";
import type { ConflictFile } from "../src/lib/types.ts";

function deferred<T>() { let resolve!: (v: T) => void, reject!: (e: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const file = (stages: string, worktree: string): ConflictFile => ({ path: "a.txt", stages, worktree, text: true, base: true, ours: true, theirs: true, binary: false, link: false, renamed: null, openEnd: false, chunks: [{ kind: "conflict", ours: ["ours"], base: ["base"], theirs: ["theirs"] }] });

// Run the component's real async file loader with controllable API completions.
function loader(state = takeDraft(null, "merge", ["a.txt"])) {
  const requests: ReturnType<typeof deferred<ConflictFile>>[] = [];
  const api = { conflictFile: () => { const d = deferred<ConflictFile>(); requests.push(d); return d.promise; } };
  const source = fs.readFileSync(new URL("../src/lib/ConflictsView.svelte", import.meta.url), "utf8");
  const start = source.indexOf("  async function open(");
  const end = source.indexOf("  $effect", start);
  const js = ts.transpile(source.slice(start, end), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const value = Function("state", "api", "hasWork", "workingText", `
    let {choices, texts, stages, results, old} = state;
    let openTicket = 0, current = null, file = null, error = null;
    let at = -1, reach = null, leaving = false, fresh = null, comparing = false, draftAt = 1000;
    const textBox = null, writeResult = () => {};
    ${js}
    return { open, get current() { return current; }, get file() { return file; }, get error() { return error; },
      get state() { return {...state, choices, texts, stages, results, old}; } };
  `)(state, api, hasWork, workingText);
  return { value, requests };
}

// Partial external work is the initial result. Restart keeps a fitting draft; an external
// edit with unchanged index stages invalidates that draft and is shown instead.
{
  const l = loader();
  const first = l.value.open("a.txt");
  l.requests[0].resolve(file("index\nworktree:one", "partial\r\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> topic\n"));
  await first;
  const saved = l.value.state as DraftState;
  assert.equal(saved.texts.get("a.txt")?.text, l.value.file.worktree);
  saved.texts.set("a.txt", { text: "draft edit\r\n", start: l.value.file.worktree, stray: 0 });
  const disk = readDraft(JSON.parse(JSON.stringify(makeDraft("merge", saved, ["a.txt"], 5000))));
  const restarted = loader(takeDraft(disk, "merge", ["a.txt"]));
  const same = restarted.value.open("a.txt");
  restarted.requests[0].resolve(file("index\nworktree:one", "old working text"));
  await same;
  assert.equal(restarted.value.state.texts.get("a.txt").text, "draft edit\r\n");
  const refresh = restarted.value.open("a.txt", true);
  restarted.requests[1].resolve(file("index\nworktree:two", "new external solution\r\n"));
  await refresh;
  assert.equal(restarted.value.state.texts.get("a.txt").text, "new external solution\r\n");
  assert.equal(restarted.value.state.old.get("a.txt").result, "draft edit\r\n");
}

// Out-of-order reads of the same path cannot replace a newer version, or resurrect a file
// after an external git add removed it from the conflicted list.
for (const resolved of [false, true]) {
  const l = loader();
  const old = l.value.open("a.txt");
  const fresh = l.value.open(resolved ? null : "a.txt", true);
  if (!resolved) l.requests[1].resolve(file("new", "new result"));
  await fresh;
  l.requests[0].resolve(file("old", "obsolete result"));
  await old;
  assert.equal(l.value.file?.stages ?? null, resolved ? null : "new");
  assert.equal(l.value.current, resolved ? null : "a.txt");
}
