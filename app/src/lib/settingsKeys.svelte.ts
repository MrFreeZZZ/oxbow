// Settings › Keyboard Shortcuts: the commands by category, the screens' own keys last, with the
// section's own search.

import { keys } from "./keys";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { copy, model } from "./settingsModel.svelte";
import type { Row, Section } from "./settingsRows";
import { fits, nameOf, shortcutEdit } from "./shortcutEdit.svelte";
import { bindingsOf, changedKeys, commandIds, commandOf, defaultsOf, startsWith, strokeOf, type ShortcutId } from "./shortcuts";

export const KEYBOARD = "M1.8 4.5h12.4v7H1.8zM4 6.6h.1M6 6.6h.1M8 6.6h.1M10 6.6h.1M12 6.6h.1M4.5 9.3h7";
const KEY_GROUPS: { title: string; categories: string[]; foot?: string }[] = [
  { title: "File", categories: ["Preferences", "File"] },
  { title: "Go", categories: ["Go"] },
  { title: "Search", categories: ["Search"] },
  { title: "View", categories: ["View"] },
  { title: "Custom Commands", categories: ["Custom"] },
  { title: "Git", categories: ["Git"] },
  { title: "Changes", categories: ["Changes"], foot: "In the commit message of the Changes panel." },
  { title: "Edit Stack", categories: ["Edit Stack"], foot: "On the Edit Stack screen. A key alone works while no text field has the cursor." },
  { title: "Bisect", categories: ["Bisect"], foot: "While bisecting, in History. A key alone works while no text field has the cursor." },
];

const mac = navigator.platform.startsWith("Mac");

/** The section's own search: words, or keys pressed with Record Keys on. */
export const keySearch = $state({ query: "", stroke: null as string | null, record: false, changedOnly: false });

function keyRow(id: ShortcutId): Row {
  return {
    id: `keys:${id}`,
    label: () => (model.query.trim() ? nameOf(id) : commandOf(id).title),
    sub: () => shortcutEdit.note(id)?.text ?? "",
    subTone: () => shortcutEdit.note(id)?.tone,
    control: { type: "keys", id },
    more: () => {
      const combos = bindingsOf(id);
      const fallback = defaultsOf(id);
      return [
        ...combos.map((c): MenuEntry => ({ kind: "item", label: `Change ${keys(c)}…`, icon: menuIcons.edit, run: () => shortcutEdit.start(id, c) })),
        { kind: "item", label: combos.length ? "Add Another Key…" : "Add Key…", icon: "M8 3.5v9M3.5 8h9", run: () => shortcutEdit.start(id) },
        ...combos.map((c): MenuEntry => ({ kind: "item", label: `Remove ${keys(c)}`, icon: menuIcons.drop, run: () => shortcutEdit.remove(id, c) })),
        { kind: "sep" },
        { kind: "item", label: "Copy Command ID", icon: menuIcons.copy, run: () => copy(id, id) },
        ...(changedKeys(id) ? [{ kind: "sep" } as MenuEntry, { kind: "item", label: `Reset to Default (${fallback.length ? fallback.map((c) => keys(c)).join(", ") : "no key"})`, icon: menuIcons.reset, run: () => shortcutEdit.reset(id) } as MenuEntry] : []),
      ];
    },
  };
}

/** Record Keys: what is pressed in the search field is searched for, not typed. */
export function searchKey(event: KeyboardEvent) {
  if (!keySearch.record) return;
  const plain = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
  if (plain && event.key === "Tab") return;
  event.preventDefault();
  if (plain && event.key === "Escape") {
    keySearch.record = false;
    return;
  }
  if (plain && event.key === "Backspace") {
    keySearch.stroke = null;
    keySearch.query = "";
    return;
  }
  const stroke = strokeOf(event);
  if (!stroke) return;
  keySearch.stroke = stroke;
  keySearch.query = "";
}

export const keysSection: Section = {
  id: "keys",
  label: "Keyboard Shortcuts",
  color: "var(--section-blue)",
  icon: KEYBOARD,
  groups: () => {
    const { query, stroke, changedOnly } = keySearch;
    const shown = commandIds().filter((id) => model.query.trim() || (fits(id, query, mac) && (!stroke || bindingsOf(id).some((c) => startsWith(c, stroke))) && (!changedOnly || changedKeys(id))));
    return KEY_GROUPS.flatMap(({ title, categories, foot }) => {
      const ids = shown.filter((id) => categories.includes(commandOf(id).category));
      return ids.length ? [{ title, foot, rows: ids.map(keyRow) }] : [];
    });
  },
};
