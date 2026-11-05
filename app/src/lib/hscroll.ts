// No Wrap in a diff: every line of a file slides sideways together, as one page would. Each
// line's code clips what runs past its column, so Side by Side keeps both sides in place and
// the line numbers stay where they are. The bar under the file, a sideways swipe or Shift with
// the wheel, and Find bringing a match into view all move every line of the file at once.

/** Columns the longest line takes, with tabs `tab` columns apart, for the width lines scroll in. */
export function widestLine(texts: Iterable<string>, tab: number): number {
  let widest = 0;
  for (const text of texts) {
    let col = 0;
    for (let i = 0; i < text.length; i++) col = text.charCodeAt(i) === 9 ? col + tab - (col % tab) : col + 1;
    if (col > widest) widest = col;
  }
  return widest;
}

/** Svelte action for a file's code: keeps the `.text` of its lines at one sideways offset, and
 *  sizes its `.hbar`. */
export function hscroll(code: HTMLElement) {
  let x = 0;
  let range = 0;
  let measured = new WeakSet<HTMLElement>();
  const pending = new Set<HTMLElement>();
  let frame = 0;
  let full = false;
  const bar = () => code.querySelector<HTMLElement>(":scope > .hbar");

  function move(to: number, from: Element | null) {
    x = Math.max(0, Math.min(range, Math.round(to)));
    for (const text of code.querySelectorAll<HTMLElement>(".text")) {
      if (text !== from && text.scrollLeft !== x) text.scrollLeft = x;
    }
    const b = bar();
    if (b && b !== from && b.scrollLeft !== x) b.scrollLeft = x;
  }

  /** Read new lines once; unloaded blocks must not shrink the file's scrolling range. */
  function measure() {
    frame = 0;
    if (full) {
      measured = new WeakSet();
      range = 0;
    }
    const texts = full ? code.querySelectorAll<HTMLElement>(".text") : pending;
    for (const text of texts) {
      if (!code.contains(text) || measured.has(text)) continue;
      range = Math.max(range, text.scrollWidth - text.clientWidth);
      measured.add(text);
    }
    full = false;
    pending.clear();
    const b = bar();
    if (b) {
      b.hidden = range === 0;
      (b.firstElementChild as HTMLElement | null)?.style.setProperty("width", `${b.clientWidth + range}px`);
    }
    move(x, null);
  }

  function schedule(reset = false) {
    full ||= reset;
    if (!frame) frame = requestAnimationFrame(measure);
  }

  function onWheel(event: WheelEvent) {
    if (!range) return;
    const sideways = Math.abs(event.deltaX) > Math.abs(event.deltaY);
    // Shift turns the wheel sideways; the Mac does that itself, other systems may not.
    const dx = sideways ? event.deltaX : event.shiftKey ? event.deltaY : 0;
    if (!dx) return;
    event.preventDefault();
    move(x + dx, null);
  }

  // A line scrolled by the browser itself, as Find brings a match into view, or the bar dragged.
  function onScroll(event: Event) {
    const target = event.target as HTMLElement;
    if (target === code || target.scrollLeft === x) return;
    if (target.classList.contains("text") || target.classList.contains("hbar")) move(target.scrollLeft, target);
  }

  // Lines drawn as they come near the screen start where the others are.
  const added = new MutationObserver((records) => {
    for (const record of records) for (const node of record.addedNodes) {
      if (node.nodeType !== 1) continue;
      const el = node as HTMLElement;
      if (el.matches(".text")) pending.add(el);
      for (const text of el.querySelectorAll<HTMLElement>(".text")) pending.add(text);
    }
    schedule();
  });
  added.observe(code, { childList: true, subtree: true });
  let width: number | undefined;
  const resized = new ResizeObserver((entries) => {
    const next = entries[0]?.contentRect.width;
    // Lazy blocks change the height as they mount; only a width change invalidates measurements.
    if (next !== width) {
      width = next;
      schedule(true);
    }
  });
  resized.observe(code);
  code.addEventListener("wheel", onWheel, { passive: false });
  code.addEventListener("scroll", onScroll, true);
  schedule(true);

  return {
    // The caller also invalidates for font, tab width and the file's column count.
    update() {
      schedule(true);
    },
    destroy() {
      cancelAnimationFrame(frame);
      pending.clear();
      added.disconnect();
      resized.disconnect();
      code.removeEventListener("wheel", onWheel);
      code.removeEventListener("scroll", onScroll, true);
    },
  };
}
