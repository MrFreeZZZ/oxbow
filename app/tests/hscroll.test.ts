import assert from "node:assert/strict";
import { test } from "node:test";
import { hscroll } from "../src/lib/hscroll.ts";

// A DOM fixture counts geometry reads: timing a mock cannot establish real scrolling speed.
function fixture() {
  let reads = 0;
  let mutation!: (records: unknown[]) => void;
  let resize!: (entries: unknown[]) => void;
  const frames = new Map<number, () => void>();
  let seq = 0;
  class Text {
    nodeType = 1;
    scrollLeft = 0;
    width = 400;
    extent = 1200;
    classList = { contains: (name: string) => name === "text" };
    matches(name: string) { return name === ".text"; }
    querySelectorAll() { return []; }
    get scrollWidth() { reads++; return this.extent; }
    get clientWidth() { reads++; return this.width; }
  }
  let texts = Array.from({ length: 256 }, () => new Text());
  const bar = { hidden: false, scrollLeft: 0, clientWidth: 400, firstElementChild: { style: { setProperty(_key: string, value: string) { barWidth = value; } } } };
  let barWidth = "";
  const listeners = new Map<string, (event: any) => void>();
  const code = {
    contains: (text: Text) => texts.includes(text),
    querySelector: () => bar,
    querySelectorAll: () => texts,
    addEventListener: (name: string, callback: (event: any) => void) => listeners.set(name, callback),
    removeEventListener: (name: string) => listeners.delete(name),
  };
  const saved = Object.fromEntries(["MutationObserver", "ResizeObserver", "requestAnimationFrame", "cancelAnimationFrame"].map((key) => [key, (globalThis as any)[key]]));
  Object.assign(globalThis, {
    MutationObserver: class { constructor(callback: typeof mutation) { mutation = callback; } observe() {} disconnect() {} },
    ResizeObserver: class { constructor(callback: typeof resize) { resize = callback; } observe() {} disconnect() {} },
    requestAnimationFrame: (callback: () => void) => { frames.set(++seq, callback); return seq; },
    cancelAnimationFrame: (id: number) => frames.delete(id),
  });
  const action = hscroll(code as unknown as HTMLElement);
  const flush = () => { const pending = [...frames.values()]; frames.clear(); pending.forEach((callback) => callback()); };
  return {
    action, flush, texts, bar, listeners,
    get reads() { return reads; },
    get barWidth() { return barWidth; },
    get pending() { return frames.size; },
    add() { const text = new Text(); texts.push(text); mutation([{ addedNodes: [text] }]); return text; },
    remove(text: Text) { texts = texts.filter((item) => item !== text); mutation([{ addedNodes: [], removedNodes: [text] }]); },
    resize(width: number) { texts.forEach((text) => text.width = width); resize([{ target: code, contentRect: { width } }]); },
    restore() { action.destroy(); Object.assign(globalThis, saved); },
  };
}

test("No Wrap measures added lines once and keeps the offset when blocks leave", () => {
  const f = fixture();
  try {
    f.flush();
    const initial = f.reads;
    f.listeners.get("wheel")!({ deltaX: 300, deltaY: 0, preventDefault() {} });
    const added = f.add();
    f.add();
    f.flush();
    assert.equal(added.scrollLeft, 300);
    assert.equal(f.reads - initial, 4, "only two new lines need geometry reads");
    added.extent = 1600;
    f.action.update();
    f.flush();
    assert.equal(f.barWidth, "1600px");
    f.listeners.get("wheel")!({ deltaX: 2000, deltaY: 0, preventDefault() {} });
    f.remove(added);
    f.flush();
    assert.equal(f.bar.scrollLeft, 1200, "unmounting the widest line preserves the range");
    f.resize(800);
    f.flush();
    assert.equal(f.bar.scrollLeft, 400, "a full resize clamps to the new range");
    f.texts.forEach((text) => { text.extent = 300; });
    f.action.update();
    f.flush();
    assert.equal(f.bar.hidden, true);
    assert.equal(f.bar.scrollLeft, 0);
    f.add();
    assert.ok(f.pending);
    f.action.destroy();
    assert.equal(f.pending, 0);
    assert.equal(f.listeners.size, 0);
  } finally { f.restore(); }
});
