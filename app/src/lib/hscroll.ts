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
  const bar = () => code.querySelector<HTMLElement>(":scope > .hbar");

  function move(to: number, from: Element | null) {
    x = Math.max(0, Math.min(range, Math.round(to)));
    for (const text of code.querySelectorAll<HTMLElement>(".text")) {
      if (text !== from && text.scrollLeft !== x) text.scrollLeft = x;
    }
    const b = bar();
    if (b && b !== from && b.scrollLeft !== x) b.scrollLeft = x;
  }

  /** How far the lines can slide: the widest drawn line past its column. */
  function measure() {
    let next = 0;
    for (const text of code.querySelectorAll<HTMLElement>(".text")) next = Math.max(next, text.scrollWidth - text.clientWidth);
    range = next;
    const b = bar();
    if (b) {
      b.hidden = range === 0;
      (b.firstElementChild as HTMLElement | null)?.style.setProperty("width", `${b.clientWidth + range}px`);
    }
    move(x, null);
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
  const added = new MutationObserver(() => measure());
  added.observe(code, { childList: true, subtree: true });
  const resized = new ResizeObserver(() => measure());
  resized.observe(code);
  code.addEventListener("wheel", onWheel, { passive: false });
  code.addEventListener("scroll", onScroll, true);
  measure();

  return {
    destroy() {
      added.disconnect();
      resized.disconnect();
      code.removeEventListener("wheel", onWheel);
      code.removeEventListener("scroll", onScroll, true);
    },
  };
}
