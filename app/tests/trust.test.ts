import { restrictedLines, settingsTag, trustWhy } from "../src/lib/trust.ts";
import type { TrustFinding } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const found = (kind: TrustFinding["kind"], key: string): TrustFinding => ({ kind, key, value: "x", origin: ".git/config" });

test("each setting says when it runs", () => {
  assert.equal(trustWhy(found("hook", "hooks/post-checkout")), "Runs after switching branches");
  assert.equal(trustWhy(found("hook", "hooks/p4-pre-submit")), "Runs when git gets to that step");
  assert.equal(trustWhy(found("hook", "core.hooksPath")), "Hooks run from this folder");
  assert.equal(trustWhy(found("command", "submodule.lib.update")), "Runs when updating this submodule");
});

test("the popover puts the hooks on one line, after the settings", () => {
  const lines = restrictedLines([found("hook", "hooks/pre-commit"), found("ssh", "core.sshCommand"), found("hook", "hooks/pre-push"), found("hook", "core.hooksPath")]);
  assert.deepEqual(lines, [
    { label: "core.sshCommand", as: "your ssh" },
    { label: "core.hooksPath", as: "off" },
    { label: "Hooks (2)", as: "off" },
  ]);
});

test("Settings names two settings at most", () => {
  assert.equal(settingsTag(["hooks/pre-commit", "hooks/commit-msg"]), "hooks");
  assert.equal(settingsTag(["core.hooksPath", "hooks/pre-commit", "filter.age.clean", "core.sshCommand"]), "core.hooksPath, hooks +2");
});
