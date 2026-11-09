import type { History } from "./types";

/** The commits at `starts` and every commit before them, as far as History has loaded. */
export function reachable(history: History, starts: string[]): Set<string> {
  const byId = new Map(history.rows.map((r) => [r.id, r]));
  const seen = new Set<string>();
  const stack = [...starts];
  while (stack.length) {
    const next = stack.pop()!;
    if (seen.has(next)) continue;
    seen.add(next);
    stack.push(...(byId.get(next)?.parents ?? []));
  }
  return seen;
}

/** `id` and every commit before it. */
export const ancestors = (history: History, id: string | null) => reachable(history, id ? [id] : []);
