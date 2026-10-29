// The menu of a version of Local History, in File History and on the Local History screen.

import { save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { confirm } from "./confirm.svelte";
import { NEWEST_SAVED, restoreRequest, whenSaved } from "./localHistory";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { nav } from "./nav.svelte";
import type { LocalVersion } from "./types";

/** What the menu needs from the view it is in. */
export interface LocalVersionContext {
  /** Compare with Current File, in the view; null where the view has no such thing. */
  compare: (() => void) | null;
  /** File History of the file: on the Local History screen, not in File History itself. */
  history: boolean;
}

const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** Save As: the version written where another app can open it. */
export async function saveVersionAs(v: LocalVersion) {
  const name = nameOf(v.path);
  const to = await save({ title: `Save ${name} as it was saved ${whenSaved(v.at)}`, defaultPath: name });
  if (!to) return;
  try {
    await api.saveLocalVersion(v.id, to);
    confirm.say(`Saved the version of ${name}.`, "done");
  } catch (err) {
    confirm.say(String(err), "error");
  }
}

/** Restore This Version…, through the sheet. */
export const restoreVersion = (v: LocalVersion) => confirm.runner?.(restoreRequest(v, whenSaved(v.at)));

export function localVersionEntries(v: LocalVersion, ctx: LocalVersionContext): MenuEntry[] {
  const gone = "Local History no longer has this version";
  const entries: MenuEntry[] = [
    { kind: "item", label: "Restore This Version…", icon: menuIcons.checkout, run: () => restoreVersion(v), off: !v.kept, detail: v.kept ? undefined : gone },
  ];
  const compare = ctx.compare;
  if (compare) entries.push({ kind: "item", label: "Compare with Current File", icon: menuIcons.compare, run: compare, off: !v.kept });
  entries.push({ kind: "item", label: "Save As…", icon: menuIcons.patch, run: () => void saveVersionAs(v), off: !v.kept });
  entries.push({ kind: "sep" });
  if (ctx.history) entries.push({ kind: "item", label: "File History", icon: menuIcons.history, run: () => nav.openFile(v.path, NEWEST_SAVED) });
  if (!v.gone) entries.push({ kind: "item", label: "Open in Editor", icon: menuIcons.open, run: () => api.openInEditor(v.path, null).catch((err) => confirm.fail(err)) });
  entries.push({ kind: "item", label: "Copy Path", icon: menuIcons.copy, run: () => navigator.clipboard.writeText(v.path).then(() => confirm.say(`Copied ${v.path}.`, "done"), () => {}) });
  return entries;
}
