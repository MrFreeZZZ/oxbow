import assert from "node:assert/strict";
import test from "node:test";
import { settingsRoute, settingsSection } from "../src/lib/settingsNavigation.ts";

test("first Settings load and an existing window resolve the same destination", () => {
  const destinations = {
    updates: { section: "general" },
    commands: { section: "commands" },
    general: { section: "general" },
    "repository-tab-tags": { section: "theme", group: "repository-tab-tags" },
  };
  for (const [target, destination] of Object.entries(destinations)) {
    assert.deepEqual(settingsRoute(`#settings/${target}`), destination);
    assert.deepEqual(settingsSection(target), destination);
  }
  assert.deepEqual(settingsRoute("#settings"), destinations.general);
});

test("other windows and unknown destinations do not mount Settings", () => {
  for (const hash of ["", "#console", "#command", "#settings/unknown", "#settings-other"]) {
    assert.equal(settingsRoute(hash), null);
  }
  assert.equal(settingsSection("unknown"), null);
});
