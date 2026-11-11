// Window popovers share one Escape handler so the latest one closes before the view underneath.
const open: { node: HTMLElement; close: () => void }[] = [];

function onKey(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.defaultPrevented || document.querySelector('[role="menu"]')) return;
  const top = open.at(-1);
  const modal = document.querySelector('[aria-modal="true"]');
  if (!top || (modal && !modal.contains(top.node))) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  top.close();
}

export function popoverEscape(node: HTMLElement, close: () => void) {
  const entry = { node, close };
  if (!open.length) window.addEventListener("keydown", onKey, true);
  open.push(entry);
  return {
    update(close: () => void) {
      entry.close = close;
    },
    destroy() {
      open.splice(open.indexOf(entry), 1);
      if (!open.length) window.removeEventListener("keydown", onKey, true);
    },
  };
}
