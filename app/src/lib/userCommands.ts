/** Personal scripts; repository context is passed in environment variables by the core. */
import type { UserCommand } from "./types";
export type { UserCommand } from "./types";

export function commandsProblem(value: unknown[]): string | null {
  const ids = new Set<string>();
  for (const item of value) {
    const c = item as UserCommand | null;
    if (!c || [c.id, c.name, c.script, c.repository].some((v) => typeof v !== "string" || v.includes("\0")) || !/^[a-zA-Z0-9-]+$/.test(c.id) || !c.name.trim() || !c.script.trim() || c.name.length > 200 || c.script.length > 65536 || ids.has(c.id)) return "Commands need unique IDs, names and scripts without NUL characters";
    ids.add(c.id);
  }
  return null;
}

export const commandId = (id: string): `custom:${string}` => `custom:${id}`;
export const commandsFor = (commands: UserCommand[], path: string) => commands.filter((c) => !c.repository || c.repository === path);
