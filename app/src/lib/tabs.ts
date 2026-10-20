// File tabs step by their list, not by the tabs drawn: thousands of files draw only the ones
// around the open tab, so the first and the last file need not be on screen.

/** The tab `by` from the open one, Summary (null) first and then `paths` in order; undefined
 *  when it would go past either end and `wrap` is off. */
export function nextTab(paths: string[], open: string | null, by: 1 | -1, wrap = true): string | null | undefined {
  const count = paths.length + 1;
  const at = open === null ? 0 : paths.indexOf(open) + 1;
  const to = at + by;
  if (!wrap && (to < 0 || to >= count)) return undefined;
  const next = (to + count) % count;
  return next === 0 ? null : paths[next - 1];
}

/** How a tab list steps: moves the open tab by `by` and says whether it moved. */
export type TabStepper = (by: 1 | -1, wrap: boolean) => boolean;

const steppers = new WeakMap<Element, TabStepper>();

/** `use:tabStepper={step}` on a `role="tablist"`: ⌥⌘→, ⌥⌘← and the arrow keys go through it. */
export function tabStepper(node: HTMLElement, step: TabStepper) {
  steppers.set(node, step);
  return {
    update(next: TabStepper) {
      steppers.set(node, next);
    },
    destroy() {
      steppers.delete(node);
    },
  };
}

export function stepperOf(list: Element | null | undefined): TabStepper | undefined {
  return list ? steppers.get(list) : undefined;
}

/** The stepper for a component's file tabs: Summary and then `paths`, opened by `open`. */
export function fileTabs(paths: string[], tab: string | null, open: (path: string | null) => void): TabStepper {
  return (by, wrap) => {
    const next = nextTab(paths, tab, by, wrap);
    if (next === undefined) return false;
    open(next);
    return true;
  };
}
