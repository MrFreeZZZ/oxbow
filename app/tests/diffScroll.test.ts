import assert from "node:assert/strict";
import { test } from "node:test";
import { dismissPreviewOnEscape } from "../src/lib/diffScroll.ts";

function escapeAt(target: { tagName: string; isContentEditable?: boolean }) {
  const event = new Event("keydown", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "target", { value: target });
  return Object.assign(event, { key: "Escape" }) as KeyboardEvent;
}

test("the preview leaves Escape to focused fields and editors", () => {
  for (const target of [{ tagName: "INPUT" }, { tagName: "TEXTAREA" }, { tagName: "SPAN", isContentEditable: true }]) {
    const event = escapeAt(target);
    dismissPreviewOnEscape(event, () => assert.fail("The field owns Escape"));
    assert.equal(event.defaultPrevented, false);
    assert.equal(event.cancelBubble, false);
  }
});

test("Escape closes the preview and cannot reach the surrounding view", () => {
  const event = escapeAt({ tagName: "BUTTON" });
  let dismissed = false;
  dismissPreviewOnEscape(event, () => { dismissed = true; });
  assert.equal(dismissed, true);
  assert.equal(event.defaultPrevented, true);
  assert.equal(event.cancelBubble, true);
});
