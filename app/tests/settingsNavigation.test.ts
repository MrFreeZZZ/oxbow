import assert from "node:assert/strict";
import test from "node:test";
import { settingsRoute, settingsSection } from "../src/lib/settingsNavigation.ts";

test("first Settings load and an existing window resolve the same destination", () => {
  for (const [target, section] of [["updates", "general"], ["commands", "commands"], ["general", "general"]]) {
    assert.equal(settingsRoute(`#settings/${target}`), section);
    assert.equal(settingsSection(target), section);
  }
  assert.equal(settingsRoute("#settings"), "general");
});

test("other windows and unknown destinations do not mount Settings", () => {
  for (const hash of ["", "#console", "#command", "#settings/unknown", "#settings-other"]) {
    assert.equal(settingsRoute(hash), null);
  }
  assert.equal(settingsSection("unknown"), null);
});
