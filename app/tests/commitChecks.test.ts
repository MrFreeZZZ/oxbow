import { checkCommands, checkFailed, checkProblem, checkRecovery, checkSub, movedCheck, withCheck } from "../src/lib/commitChecks.ts";
import { trustWhy } from "../src/lib/trust.ts";
import type { Action, CommitCheck, Failure } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const list: CommitCheck[] = [
  { command: "ruff check .", on: true },
  { command: "npm test", on: false },
];

test("the list is written whole, a check turned off with #", () => {
  assert.deepEqual(checkCommands(list), [
    "git config --local --unset-all oxbow.check",
    'git config --local --add oxbow.check "ruff check ."',
    'git config --local --add oxbow.check "# npm test"',
  ]);
  assert.deepEqual(checkCommands([]), ["git config --local --unset-all oxbow.check"]);
  assert.deepEqual(checkCommands([{ command: "make", on: true }]).at(-1), "git config --local --add oxbow.check make");
});

test("a check needs a command of its own", () => {
  assert.equal(checkProblem(list, null, "  "), "Type a command");
  assert.equal(checkProblem(list, null, "npm test"), "This check is in the list already");
  assert.equal(checkProblem(list, 1, "npm test"), null, "editing keeps its own command");
  assert.match(checkProblem(list, null, "#lint") ?? "", /can’t start with #/);
  assert.equal(checkProblem(list, null, "cargo fmt --check"), null);
});

test("adding, editing and moving keep the others as they were", () => {
  assert.deepEqual(withCheck(list, null, " cargo test "), [...list, { command: "cargo test", on: true }]);
  assert.deepEqual(withCheck(list, 1, "npm run test"), [list[0], { command: "npm run test", on: false }]);
  assert.deepEqual(movedCheck(list, 1, -1), [list[1], list[0]]);
  assert.deepEqual(movedCheck(list, 0, -1), list, "the first stays first");
  assert.deepEqual(movedCheck(list, 1, 1), list, "the last stays last");
  const moved = movedCheck(list, 0, 1);
  moved[0].on = true;
  assert.equal(list[1].on, false, "the list given is not changed");
});

test("a row says whether its check runs", () => {
  assert.match(checkSub(list[0], false), /^Runs before each commit/);
  assert.match(checkSub(list[1], false), /^Off/);
  assert.match(checkSub(list[0], true), /Restricted/);
  assert.equal(trustWhy({ kind: "check", key: "oxbow.check", value: "npm test", origin: ".git/config" }), "Oxbow runs it before each commit");
});

test("a failed check offers Try Again and Commit Anyway, which skips the checks once", async () => {
  const action: Action & { kind: "commit" } = { kind: "commit", message: "Add banner", amend: false };
  const failure: Failure = { kind: "check", output: "1 error", incoming: [], remoteTip: null, hook: null, check: "ruff check ." };
  const seen: Action[] = [];
  const again = (_: Failure, next: Action & { kind: "commit" }) => (seen.push(next), null);
  const recovery = checkRecovery(failure, action, "Add banner", "main", again);
  assert.equal(recovery.title, "A check before commit failed");
  assert.ok(recovery.body.some((part) => typeof part === "object" && "code" in part && part.code === "ruff check ."));
  assert.equal(recovery.button?.label, "Try Again");
  assert.deepEqual(recovery.button?.action, action);
  recovery.button?.recover?.(failure);
  assert.equal(recovery.alt?.label, "Commit Anyway…");
  const anyway = await recovery.alt!.request();
  assert.equal(anyway.button, "Commit Anyway");
  assert.deepEqual(anyway.action, { ...action, skipChecks: true });
  // Should a hook then stop it, its way out keeps the checks skipped.
  anyway.recover?.({ ...failure, kind: "hook" });
  assert.deepEqual(seen, [action, { ...action, skipChecks: true }]);

  const amend = checkRecovery(failure, { ...action, amend: true }, "Add banner", "main", again);
  assert.equal(amend.alt?.label, "Amend Anyway…");
  assert.match(String(checkFailed(failure, "making the fixup commit").body.at(-1)), /before making the fixup commit/);
});
