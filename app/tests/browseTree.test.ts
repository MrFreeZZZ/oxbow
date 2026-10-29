import { findFiles, foldersOf, leftOf, linesOf, stepRow, treeRows } from "../src/lib/browseTree.ts";
import type { TreeItem } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const folder = (path: string): TreeItem => ({ name: path.split("/").pop()!, path, kind: "folder", size: 0 });
const file = (path: string): TreeItem => ({ name: path.split("/").pop()!, path, kind: "file", size: 10 });

const folders = new Map<string, TreeItem[]>([
  ["", [folder("src"), file("README.md")]],
  ["src", [folder("src/bin"), file("src/lib.rs")]],
  ["src/bin", [file("src/bin/tool.sh")]],
]);

test("an open folder shows its entries under it, one level deeper", () => {
  const rows = (open: string[]) => treeRows(folders, new Set(open)).map((r) => `${"  ".repeat(r.depth)}${r.item.name}`);
  assert.deepEqual(rows([]), ["src", "README.md"]);
  assert.deepEqual(rows(["src"]), ["src", "  bin", "  lib.rs", "README.md"]);
  assert.deepEqual(rows(["src", "src/bin"]), ["src", "  bin", "    tool.sh", "  lib.rs", "README.md"]);
  // A folder open inside a closed one stays hidden.
  assert.deepEqual(rows(["src/bin"]), ["src", "README.md"]);
});

test("a file's folders open from the top down", () => {
  assert.deepEqual(foldersOf("src/bin/tool.sh"), ["src", "src/bin"]);
  assert.deepEqual(foldersOf("README.md"), []);
});

test("finding a file takes its words in order, names first", () => {
  const paths = ["docs/limit.md", "src/limit.rs", "src/routes.rs", "tests/limit_test.rs"];
  assert.deepEqual(findFiles(paths, "limit"), ["docs/limit.md", "src/limit.rs", "tests/limit_test.rs"]);
  assert.deepEqual(findFiles(paths, "src rs"), ["src/limit.rs", "src/routes.rs"]);
  assert.deepEqual(findFiles(paths, "rs src"), []);
  assert.deepEqual(findFiles(paths, "LIMIT", 1), ["docs/limit.md"]);
  assert.deepEqual(findFiles(paths, "  "), []);
  // A path that only has the word in a folder comes after the ones named by it.
  assert.deepEqual(findFiles(["limit/a.rs", "b/limit.rs"], "limit"), ["b/limit.rs", "limit/a.rs"]);
});

test("the arrows step through the rows and stop at the ends", () => {
  const paths = ["a", "b", "c"];
  assert.equal(stepRow(paths, "b", 1), "c");
  assert.equal(stepRow(paths, "c", 1), "c");
  assert.equal(stepRow(paths, "a", -1), "a");
  assert.equal(stepRow(paths, null, 1), "a");
  assert.equal(stepRow(paths, null, -1), "c");
  assert.equal(stepRow([], null, 1), null);
});

test("← closes an open folder, else goes up to its folder", () => {
  const open = new Set(["src"]);
  assert.deepEqual(leftOf({ item: folder("src"), depth: 0 }, open), { close: "src" });
  assert.deepEqual(leftOf({ item: file("src/lib.rs"), depth: 1 }, open), { go: "src" });
  assert.equal(leftOf({ item: file("README.md"), depth: 0 }, open), null);
});

test("a file's lines leave out the end of the last one", () => {
  assert.deepEqual(linesOf("a\nb\n"), ["a", "b"]);
  assert.deepEqual(linesOf("a\r\nb"), ["a", "b"]);
  assert.deepEqual(linesOf(""), [""]);
});
