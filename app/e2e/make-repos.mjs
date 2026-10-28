// The repositories of the Mac checklist in docs/acceptance.md, the same the cloud scenarios use:
// `node app/e2e/make-repos.mjs ~/oxbow-acceptance`. Each run makes them afresh.

import path from "node:path";
import { git, lines, read, rebaseInConflict, repo, submodulesOneFails, withWorktree, write } from "./fixtures.mjs";

const dest = process.argv[2];
if (!dest) {
  console.error("usage: node app/e2e/make-repos.mjs <folder>");
  process.exit(2);
}
const at = (name) => path.join(path.resolve(dest), name);

repo(at("history"), 30);

const staging = repo(at("staging"), 1);
write(staging, "src/f1.txt", read(staging, "src/f1.txt").replace("file 1 3\nfile 1 4", "X3\nX4").replace("file 1 15", "X15"));

rebaseInConflict(at("conflicts"));

withWorktree(at("worktrees"));
withWorktree(at("gone"));

submodulesOneFails(at("submodules"));

const undo = repo(at("undo"), 2);
write(undo, "src/f1.txt", "changed by hand\n");
write(undo, "new.txt", "a new file\n");
write(undo, "src/f2.txt", lines(5, "staged"));
git(undo, "add", "src/f2.txt");

const keys = repo(at("keys"), 3);
write(keys, "src/f1.txt", "changed\n");

console.log(`made in ${path.resolve(dest)}: history staging conflicts worktrees gone submodules undo keys`);
