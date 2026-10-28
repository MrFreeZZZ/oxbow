import { commitFileActions, commitFileRequest, revertFileRequest, takeVersionRequest } from "../src/lib/commitFiles.ts";
import assert from "node:assert/strict";

const sha = "0123456789abcdef0123456789abcdef01234567";
const file = (status: "modified" | "added" | "deleted" | "renamed", more: object = {}) => ({ path: "src/app.py", status, ...more });
const ids = (list: { id: string }[]) => list.map((a) => a.id);

// A commit on the checked-out branch can have its file reverted; one elsewhere, copied. Unknown: both.
{
  assert.deepEqual(ids(commitFileActions(file("modified"), true)), ["revertFile", "takeVersion", "compare"]);
  assert.deepEqual(ids(commitFileActions(file("modified"), false)), ["cherryPickFile", "takeVersion", "compare"]);
  assert.deepEqual(ids(commitFileActions(file("modified"), null)), ["revertFile", "cherryPickFile", "takeVersion", "compare"]);
}

// A file the commit deleted has no version to take or compare; a submodule has nothing.
{
  assert.deepEqual(ids(commitFileActions(file("deleted"), true)), ["revertFile"]);
  assert.deepEqual(commitFileActions(file("modified", { submodule: { old: null, new: "abc" } }), null), []);
}

// The steps name the file and the commit, and carry a rename's old path.
{
  const revert = revertFileRequest(file("renamed", { oldPath: "app.py" }), sha, "Move the app");
  assert.deepEqual(revert.action, { kind: "revertFile", commit: sha, path: "src/app.py", oldPath: "app.py" });
  assert.equal(revert.title, "Revert app.py of 0123456?");
  assert.ok(!revert.danger);
  const pick = commitFileRequest("cherryPickFile", file("modified"), sha, null);
  assert.deepEqual(pick.action, { kind: "cherryPickFile", commit: sha, path: "src/app.py", oldPath: null });
  assert.equal(pick.title, "Cherry-pick app.py from 0123456?");
  const take = takeVersionRequest(file("modified"), sha, null);
  assert.deepEqual(take.action, { kind: "takeVersion", commit: sha, path: "src/app.py" });
  assert.match(take.title, /^Do you really want to take app\.py as it is in 0123456\?$/);
  assert.equal(commitFileRequest("takeVersion", file("modified"), sha, null).title, take.title);
}

// Reverting an added file deletes it, a deleted one comes back: the sheet says so.
{
  const words = (parts: unknown[]) => parts.map((p) => (typeof p === "string" ? p : Object.values(p as object)[0])).join("");
  assert.match(words(revertFileRequest(file("added"), sha, null).body), /deleted from your files/);
  assert.match(words(revertFileRequest(file("deleted"), sha, null).body), /comes back/);
}
