// Sheets keep the keyboard: Tab and Shift+Tab go round the sheet's own controls, and once the
// sheet closes the focus goes back to where it was before it opened, such as the button that
// opened it.

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])';

export function modal(node: HTMLElement) {
  const opener = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;

  function onKey(event: KeyboardEvent) {
    // Shift+Tab comes as ISO_Left_Tab on Linux, which names no key; its code is still Tab.
    if ((event.key !== "Tab" && event.code !== "Tab") || event.metaKey || event.ctrlKey || event.altKey) return;
    const items = [...node.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null);
    if (!items.length) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    const next = event.shiftKey ? (at <= 0 ? items.length - 1 : at - 1) : at === -1 || at === items.length - 1 ? 0 : at + 1;
    // Within the sheet the browser's own order is right; only the ends wrap round.
    if (at !== -1 && next === at + (event.shiftKey ? -1 : 1)) return;
    event.preventDefault();
    items[next].focus();
  }

  node.addEventListener("keydown", onKey);

  return {
    destroy() {
      node.removeEventListener("keydown", onKey);
      const lost = document.activeElement === document.body || document.activeElement === null || node.contains(document.activeElement);
      if (opener?.isConnected && lost) requestAnimationFrame(() => opener.focus({ preventScroll: true }));
    },
  };
}
