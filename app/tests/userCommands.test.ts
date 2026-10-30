import assert from "node:assert/strict";
import { test } from "node:test";
import { commandsFor, commandsProblem, commandId } from "../src/lib/userCommands.ts";
import { useUserCommands, commandIds, commandOf, Keymap, conflicts, type Custom } from "../src/lib/shortcuts.ts";

test("personal commands are validated and scoped by the exact repository path", () => {
  const global = { id: "a", name: "Tests", script: "cargo test", repository: "" };
  const local = { ...global, id: "b", repository: "/repo with spaces" };
  assert.equal(commandsProblem([global, local]), null);
  assert.ok(commandsProblem([global, global]));
  assert.ok(commandsProblem([{ ...global, script: "\0" }]));
  assert.ok(commandsProblem([null]));
  assert.deepEqual(commandsFor([global, local], "/other"), [global]);
  assert.deepEqual(commandsFor([global, local], local.repository), [global, local]);
});

test("custom commands use the shared keymap and conflict detection, including live additions", () => {
  let commands = [{ id: "a", name: "Tests" }];
  useUserCommands(() => commands);
  const id = commandId("a");
  const mine: Custom = { [id]: ["Mod+Shift+Y"] };
  const keymap = new Keymap(true, () => mine);
  const key = { key: "Y", code: "KeyY", metaKey: true, shiftKey: true, ctrlKey: false, altKey: false } as KeyboardEvent;
  assert.equal(commandOf(id).title, "Tests");
  assert.ok(commandIds().includes(id));
  assert.deepEqual(keymap.match(key), { id });
  assert.ok(conflicts("settings", "Mod+Shift+Y", mine, true).some((c) => c.id === id));
  commands = [];
  assert.equal(keymap.match(key), null);
  commands = [{ id: "a", name: "Build" }];
  assert.deepEqual(keymap.match(key), { id });
  assert.equal(commandOf(id).title, "Build");
  useUserCommands(() => []);
});
