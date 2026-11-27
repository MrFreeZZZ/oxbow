import assert from "node:assert/strict";
import { test } from "node:test";
import { popoverEscape } from "../src/lib/popoverEscape.ts";

type Node = { contains: (other: unknown) => boolean };

function page() {
  const listeners = new Set<(event: KeyboardEvent) => void>();
  const found: Record<string, Node | null> = { '[role="menu"]:not([inert] *)': null, '[aria-modal="true"]:not([inert] *)': null };
  const g = globalThis as Record<string, unknown>;
  g.window = {
    addEventListener: (_: string, listener: (event: KeyboardEvent) => void) => listeners.add(listener),
    removeEventListener: (_: string, listener: (event: KeyboardEvent) => void) => listeners.delete(listener),
  };
  g.document = { querySelector: (selector: string) => found[selector] ?? null };
  const press = (key = "Escape") => {
    const event = {
      key,
      defaultPrevented: false,
      preventDefault() { this.defaultPrevented = true; },
      stopImmediatePropagation() {},
    };
    for (const listener of listeners) listener(event as unknown as KeyboardEvent);
    return event.defaultPrevented;
  };
  return { listeners, found, press };
}

const node = (inside: Node[] = [], inert = false): Node & HTMLElement =>
  ({ closest: () => inert ? {} : null, contains: (other: unknown) => inside.includes(other as Node) }) as unknown as Node & HTMLElement;

test("Escape closes the latest popover first, then the one under it", () => {
  const { listeners, press } = page();
  const closed: string[] = [];
  const under = popoverEscape(node(), () => closed.push("under"));
  const top = popoverEscape(node(), () => closed.push("top"));
  assert.equal(press("Enter"), false);
  assert.equal(press(), true);
  assert.deepEqual(closed, ["top"]);
  top.destroy();
  assert.equal(press(), true);
  assert.deepEqual(closed, ["top", "under"]);
  under.destroy();
  assert.equal(listeners.size, 0);
});

test("an open menu or another dialog keeps its own Escape", () => {
  const { found, press } = page();
  const closed: string[] = [];
  const popover = node();
  const handle = popoverEscape(popover, () => closed.push("popover"));
  found['[role="menu"]:not([inert] *)'] = node();
  assert.equal(press(), false);
  found['[role="menu"]:not([inert] *)'] = null;
  found['[aria-modal="true"]:not([inert] *)'] = node();
  assert.equal(press(), false);
  found['[aria-modal="true"]:not([inert] *)'] = node([popover]);
  assert.equal(press(), true);
  assert.deepEqual(closed, ["popover"]);
  handle.destroy();
});

test("Escape ignores a newer popover in a hidden tab", () => {
  const { press } = page();
  const closed: string[] = [];
  const active = popoverEscape(node(), () => closed.push("active"));
  const hidden = popoverEscape(node([], true), () => closed.push("hidden"));
  assert.equal(press(), true);
  assert.deepEqual(closed, ["active"]);
  hidden.destroy();
  active.destroy();
});
