// What the Settings window is made of: sections on the left, each a list of titled groups of
// rows, and a row's control and where its value lives.

import type { MenuEntry } from "./Menu.svelte";
import type { PrefKey } from "./prefs.svelte";
import type { ShortcutId } from "./shortcuts";
import type { ConfigScope } from "./types";

export const GEAR =
  "M12.78 6.52L14.44 6.56L14.44 9.44L12.78 9.48L12.42 10.33L13.57 11.54L11.54 13.57L10.33 12.42L9.48 12.78L9.44 14.44L6.56 14.44L6.52 12.78L5.67 12.42L4.46 13.57L2.43 11.54L3.58 10.33L3.22 9.48L1.56 9.44L1.56 6.56L3.22 6.52L3.58 5.67L2.43 4.46L4.46 2.43L5.67 3.58L6.52 3.22L6.56 1.56L9.44 1.56L9.48 3.22L10.33 3.58L11.54 2.43L13.57 4.46L12.42 5.67zM8 5.8a2.2 2.2 0 1 0 0 4.4a2.2 2.2 0 1 0 0-4.4";

export type Value = string | number | boolean;
export type Option = { value: Value; label: string };
export type Text = string | (() => string);

/** Where a row's value lives, for its gear menu. */
export type Setting =
  | { kind: "pref"; key: PrefKey }
  | { kind: "git"; scope: ConfigScope; keys: string[]; commands: () => string[]; reset: () => Promise<void> };

export type Control =
  | { type: "value"; text: Text }
  | { type: "seg" | "popup"; options: () => Option[]; get: () => Value; set: (value: Value) => void }
  | { type: "switch"; get: () => boolean; set: (on: boolean) => void }
  /** A value with Edit, which turns it into a text box. */
  | { type: "edit"; get: () => string; shown?: () => string; placeholder?: string; save: (value: string) => void }
  | { type: "button"; label: Text; run: () => void; danger?: boolean }
  /** A command's keyboard shortcuts, with the recorder for new ones. */
  | { type: "keys"; id: ShortcutId };

export type Row = {
  id: string;
  label: Text;
  sub?: Text;
  /** The sub line as a warning or an error, e.g. a key another command has. */
  subTone?: () => "warn" | "error" | undefined;
  mono?: boolean;
  badge?: () => string | null;
  /** A round picture before the label, e.g. an account's avatar. */
  image?: () => string | null;
  /** A file's picture before the label; `missing` draws it dashed with a plus. */
  file?: () => { missing: boolean };
  control?: Control;
  setting?: Setting;
  /** Greyed out while another setting makes this one irrelevant. */
  off?: () => boolean;
  /** More items for its gear menu. */
  more?: () => MenuEntry[];
};
export type Group = { title?: string; foot?: Text; rows: Row[]; preview?: "theme" | "branches" | "diff" | "console" };
export type Section = {
  id: string;
  label: string;
  color: string;
  icon: string;
  groups: () => Group[];
  head?: string;
  /** Drawn in the sidebar under the section before it, e.g. Repository Files under the repository. */
  child?: boolean;
};

export const text = (t: Text | undefined) => (typeof t === "function" ? t() : (t ?? ""));
export const opts = (...values: (string | number)[]): Option[] => values.map((value) => ({ value, label: String(value) }));

/** The options plus the current value when it is not one of them, as one typed into
 *  settings.json can be, so the popup never shows empty. */
export function withCurrent(options: Option[], current: Value): Option[] {
  if (current === "" || options.some((o) => String(o.value) === String(current))) return options;
  const extra = { value: current as string | number, label: String(current) };
  const at = typeof current === "number" ? options.findIndex((o) => typeof o.value === "number" && o.value > current) : -1;
  return at < 0 ? [...options, extra] : [...options.slice(0, at), extra, ...options.slice(at)];
}
