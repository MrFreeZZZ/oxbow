import { browsing, splitMessage, stepMessage } from "../src/lib/messageHistory.ts";
import assert from "node:assert/strict";

const empty = { summary: "", description: "" };
const messages = ["Fix the build", "Add a test\n\nWith a body.\nOn two lines.", "Rename things"];

// A message splits at its first line; the blank line after it goes.
{
  assert.deepEqual(splitMessage("Fix the build"), { summary: "Fix the build", description: "" });
  assert.deepEqual(splitMessage("Add a test\n\nWith a body.\nOn two lines.\n"), { summary: "Add a test", description: "With a body.\nOn two lines." });
  assert.deepEqual(splitMessage("Windows\r\n\r\nline ends"), { summary: "Windows", description: "line ends" });
  assert.deepEqual(splitMessage(""), empty);
}

// ↑ in the empty box shows the newest, then older ones, and stops at the oldest; ↓ goes back
// to newer ones and then to the empty box.
{
  assert.equal(stepMessage(messages, -1, 1, empty), 0);
  assert.equal(stepMessage(messages, 0, 1, splitMessage(messages[0])), 1);
  assert.equal(stepMessage(messages, 1, 1, splitMessage(messages[1])), 2);
  assert.equal(stepMessage(messages, 2, 1, splitMessage(messages[2])), null);
  assert.equal(stepMessage(messages, 2, -1, splitMessage(messages[2])), 1);
  assert.equal(stepMessage(messages, 0, -1, splitMessage(messages[0])), -1);
  assert.equal(stepMessage(messages, -1, -1, empty), null);
}

// Text of your own, or a shown message you edited, keeps the keys for the cursor.
{
  const typed = { summary: "Fix the", description: "" };
  assert.equal(browsing(messages, -1, typed), false);
  assert.equal(stepMessage(messages, -1, 1, typed), null);
  const edited = { ...splitMessage(messages[1]), description: "With a body." };
  assert.equal(stepMessage(messages, 1, 1, edited), null);
  assert.equal(stepMessage(messages, 1, -1, edited), null);
  // Cleared again, the box starts over from the newest.
  assert.equal(stepMessage(messages, 1, 1, empty), 0);
}

// No messages yet: ↑ does nothing.
assert.equal(stepMessage([], -1, 1, empty), null);
