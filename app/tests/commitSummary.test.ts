import assert from "node:assert/strict";
import { commitSummary, summaryText } from "../src/lib/commitSummary.ts";
import type { CommitDetail, FileChange } from "../src/lib/types.ts";

const file = (path: string, status: FileChange["status"], oldPath: string | null = null): FileChange =>
  ({ path, status, oldPath, additions: 0, deletions: 0, binary: false });
const renamed = file("src/new.ts", "renamed", "src/old.ts");
const author = { name: "A. Author", email: "author@example.com", time: 0, offset: 19800 };
const detail: CommitDetail = {
  id: "0".repeat(40),
  summary: "Update files",
  body: "First paragraph.\n\nSecond paragraph.",
  author,
  committer: author,
  parents: [],
  files: [renamed, { ...file("logo.png", "added"), binary: true }, file("copy.ts", "copied", "source.ts"), file("gone.ts", "deleted")],
  shallowEnd: false,
  counted: true,
  countedFrom: 2,
};
const loaded = new Map([[renamed.path, { ...renamed, additions: 4, deletions: 2 }]]);
const date = "1 January 1970 at 05:30 (+0530)";
const context = {
  repository: "repo", path: "/work/repo", branches: "2 local · 1 remote", branch: "preview", publication: "Publication status unknown", url: null,
  remotes: [{ name: "origin", fetchUrl: "https://github.com/o/r.git", pushUrl: "git@github.com:o/r.git" }],
};
const summary = commitSummary(detail, loaded, date);
assert.deepEqual(summary.groups, [
  { name: "Added", files: [
    { path: "logo.png", origin: "", counts: null },
    { path: "copy.ts", origin: "Copied from source.ts", counts: { additions: 0, deletions: 0 } },
  ] },
  { name: "Modified", files: [
    { path: "src/new.ts", origin: "Renamed from src/old.ts", counts: { additions: 4, deletions: 2 } },
  ] },
  { name: "Deleted", files: [{ path: "gone.ts", origin: "", counts: { additions: 0, deletions: 0 } }] },
]);
assert.match(summaryText(summary, context), /^Repository: repo\nPath: \/work\/repo\nBranches: 2 local · 1 remote\nCurrent branch: preview\nRemote · origin: https:\/\/github\.com\/o\/r\.git\nPush: git@github\.com:o\/r\.git\nPublication status unknown\n\nUpdate files\n\nCommit: 0{40}\nAuthor: A\. Author <author@example\.com>\nDate: .* \(\+0530\)/);
assert.match(summaryText(summary, context), /Second paragraph\.\n\nAdded\nlogo\.png\ncopy\.ts {2}\(Copied from source\.ts\) {2}\+0 −0\n\nModified\nsrc\/new\.ts {2}\(Renamed from src\/old\.ts\) {2}\+4 −2\n\nDeleted\ngone\.ts {2}\+0 −0$/);
assert.equal(commitSummary({ ...detail, counted: false }, loaded, date).groups[2].files[0].counts, null);
assert.equal(commitSummary(detail, new Map(), date).groups[1].files[0].counts, null);
const empty = { ...detail, body: "", files: [] };
assert.match(summaryText(commitSummary(empty, loaded, date), context), /No description\.\n\nNo changes\.$/);
assert.match(summaryText(commitSummary({ ...empty, shallowEnd: true }, loaded, date), context), /No description\.\n\nThe full file list is unavailable in this shallow clone\.$/);
assert.match(summaryText(summary, { ...context, publication: "Confirmed by a recorded fetch", url: "https://github.com/o/r/commit/abc" }), /\nConfirmed by a recorded fetch\nCommit link: https:\/\/github\.com\/o\/r\/commit\/abc\n/);
assert.match(summaryText(summary, { ...context, remotes: [] }), /\nNo remotes\nPublication status unknown\n/);
assert.doesNotMatch(summaryText(summary, { ...context, publication: "Publication unverified · Found in local remote-tracking refs" }), /Commit link:/);
