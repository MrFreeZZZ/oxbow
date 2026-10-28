// The repositories the scenarios run on, made afresh in a temporary folder for each run.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const ENV = {
  ...process.env,
  GIT_CONFIG_NOSYSTEM: "1",
  GIT_AUTHOR_NAME: "Fixture",
  GIT_AUTHOR_EMAIL: "fixture@example.com",
  GIT_COMMITTER_NAME: "Fixture",
  GIT_COMMITTER_EMAIL: "fixture@example.com",
};

/** `git(dir, "commit", "-m", "x")`: runs git there and gives back its output, trimmed. */
export function git(dir, ...args) {
  return execFileSync("git", args, { cwd: dir, env: ENV, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

export function write(dir, file, text) {
  fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
  fs.writeFileSync(path.join(dir, file), text);
}

export function read(dir, file) {
  return fs.readFileSync(path.join(dir, file), "utf8");
}

/** Lines `one`…`n`, each on its own line. */
export const lines = (n, word = "line") => Array.from({ length: n }, (_, i) => `${word} ${i + 1}`).join("\n") + "\n";

/** A new repository at `dir` on main, with `count` commits: commit k changes src/fk.txt. */
export function repo(dir, count = 3) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  git(dir, "init", "-q", "-b", "main");
  for (let k = 1; k <= count; k++) {
    write(dir, `src/f${k}.txt`, lines(20, `file ${k}`));
    git(dir, "add", "-A");
    git(dir, "commit", "-q", "-m", `Commit ${k}`);
  }
  return dir;
}

/** A rebase of `feature` onto main stopped on its first of two commits, each in conflict with
 *  main in a.txt and b.txt. */
export function rebaseInConflict(dir) {
  repo(dir, 1);
  write(dir, "a.txt", lines(10, "a"));
  write(dir, "b.txt", lines(10, "b"));
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", "Base");
  git(dir, "checkout", "-q", "-b", "feature");
  for (const step of [1, 2]) {
    write(dir, "a.txt", read(dir, "a.txt").replace(/^a 2.*$/m, `a 2 feature ${step}`));
    write(dir, "b.txt", read(dir, "b.txt").replace(/^b 8.*$/m, `b 8 feature ${step}`));
    git(dir, "commit", "-q", "-am", `Feature step ${step}`);
  }
  git(dir, "checkout", "-q", "main");
  write(dir, "a.txt", read(dir, "a.txt").replace(/^a 2$/m, "a 2 main"));
  write(dir, "b.txt", read(dir, "b.txt").replace(/^b 8$/m, "b 8 main"));
  git(dir, "commit", "-q", "-am", "Main changes a and b");
  git(dir, "checkout", "-q", "feature");
  try {
    git(dir, "rebase", "main");
  } catch {
    // Stopped on the first conflict, as wanted.
  }
  return dir;
}

/** A repository with a second worktree on branch `side`, in `<dir>-side`. */
export function withWorktree(dir) {
  repo(dir, 2);
  const side = `${dir}-side`;
  fs.rmSync(side, { recursive: true, force: true });
  git(dir, "worktree", "add", "-q", "-b", "side", side);
  return { main: dir, side };
}

/** A repository with submodules lib-a, lib-b and lib-c, each cloned a commit behind the one it
 *  records, which only their sources have; lib-c's source is moved away to `<dir>-gone`, so
 *  updating it fails until it is put back. */
export function submodulesOneFails(dir) {
  const src = `${dir}-src`;
  fs.rmSync(src, { recursive: true, force: true });
  fs.rmSync(`${dir}-gone`, { recursive: true, force: true });
  repo(dir, 1);
  for (const name of ["lib-a", "lib-b", "lib-c"]) {
    repo(path.join(src, name), 1);
    git(dir, "-c", "protocol.file.allow=always", "submodule", "add", "-q", path.join(src, name), `libs/${name}`);
    // Git takes submodules from folders only when told to.
    git(path.join(dir, "libs", name), "config", "protocol.file.allow", "always");
  }
  git(dir, "config", "protocol.file.allow", "always");
  git(dir, "commit", "-q", "-m", "Add three submodules");
  for (const name of ["lib-a", "lib-b", "lib-c"]) {
    const lib = path.join(src, name);
    write(lib, "NEXT", "the next commit\n");
    git(lib, "add", "-A");
    git(lib, "commit", "-q", "-m", `Next in ${name}`);
    git(dir, "update-index", "--cacheinfo", `160000,${git(lib, "rev-parse", "HEAD")},libs/${name}`);
  }
  git(dir, "commit", "-q", "-m", "Record the next commits");
  fs.renameSync(path.join(src, "lib-c"), `${dir}-gone`);
  return { dir, gone: `${dir}-gone`, source: path.join(src, "lib-c") };
}
