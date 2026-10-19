// The Console's log as the core keeps it (oxbow_core::console), and what its window works out
// from it: which entries a filter shows, their days and times, how long each took.

export type ConsoleKind = "action" | "background" | "read";

/** A line a process printed; `e` when it went to standard error. */
export interface ConsoleLine {
  t: string;
  e?: boolean;
}

/** One process of an entry. */
export interface ConsoleRan {
  display: string;
  /** What Oxbow ran it for, e.g. "secrets check"; none for the action's own commands. */
  role?: string;
  time: number;
  millis: number;
  /** null when it was stopped or did not start. */
  code: number | null;
  stopped: boolean;
  output?: ConsoleLine[];
  running?: boolean;
}

export interface ConsoleEntry {
  id: number;
  /** Milliseconds since the epoch. */
  time: number;
  kind: ConsoleKind;
  title: string;
  millis: number;
  failed: boolean;
  stopped: boolean;
  /** Read with gitoxide inside Oxbow, without a git process. */
  gitoxide: boolean;
  note?: string;
  commands: ConsoleRan[];
  running?: boolean;
}

export interface ConsoleRepo {
  name: string;
  /** null: what ran outside any repository. */
  path: string | null;
  /** Its log file: one for all worktrees of a repository, which tells logs apart. */
  log: string | null;
  count: number;
}

export interface ConsoleRepos {
  /** The repository of the window used last. */
  current: string | null;
  repos: ConsoleRepo[];
}

/** All, My Actions, Background (the timer's fetches and the reads). */
export type ConsoleFilter = "all" | "mine" | "background";

/** How many entries a repository's log keeps, as oxbow_core::console::KEEP. */
export const CONSOLE_KEEP = 1000;
/** Of those, how many reads at most, as oxbow_core::console::KEEP_READS. */
export const CONSOLE_KEEP_READS = 300;

/** Whether `entry` is shown under `filter` with the search `needle`. */
export function shows(entry: ConsoleEntry, filter: ConsoleFilter, needle: string): boolean {
  if (filter === "mine" && entry.kind !== "action") return false;
  if (filter === "background" && entry.kind === "action") return false;
  const n = needle.trim().toLowerCase();
  if (!n) return true;
  if (entry.title.toLowerCase().includes(n) || entry.note?.toLowerCase().includes(n)) return true;
  return entry.commands.some((c) => c.display.toLowerCase().includes(n) || c.role?.toLowerCase().includes(n) || c.output?.some((l) => l.t.toLowerCase().includes(n)));
}

/** The commands the action's sheet showed, which the Console shows in full. */
export const ownCommands = (entry: ConsoleEntry) => entry.commands.filter((c) => !c.role);
/** The commands Oxbow ran for the action: its checks, the secrets check, the snapshot for Undo. */
export const helperCommands = (entry: ConsoleEntry) => entry.commands.filter((c) => !!c.role);

/** A command that went wrong: exited with another code than 0. */
export const failed = (ran: ConsoleRan) => !ran.running && !ran.stopped && ran.code !== 0;

/** The exit code to show on an entry that failed: of its first own command that did. */
export function exitOf(entry: ConsoleEntry): number | null {
  const own = entry.kind === "read" ? entry.commands : ownCommands(entry);
  return (own.find(failed) ?? entry.commands.find(failed))?.code ?? null;
}

/** "38 ms", "2.1 s", "1 min 4 s". */
export function duration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)} s`;
  if (ms < 60_000) return `${Math.round(ms / 1000)} s`;
  const min = Math.floor(ms / 60_000);
  const s = Math.round((ms % 60_000) / 1000);
  return s ? `${min} min ${s} s` : `${min} min`;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** "16:30:02", in the computer's time zone. */
export function clock(time: number): string {
  const d = new Date(time);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** The day header of `time`: "Today", "Yesterday", "Monday, 13 October", with the year when not this one's. */
export function dayOf(time: number, now: number = Date.now()): string {
  const d = new Date(time);
  const today = new Date(now);
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(today) - start(d)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  const day = `${WEEKDAYS[d.getDay()]}, ${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return d.getFullYear() === today.getFullYear() ? day : `${day} ${d.getFullYear()}`;
}

const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/** `list` with `entry` in it: in place of the one with its id, or added in time order. A running
 *  one never takes the place of the same entry done: an update can come after the log has it. */
export function upsert<T extends ConsoleEntry>(list: T[], entry: T): T[] {
  const at = list.findIndex((e) => e.id === entry.id);
  if (at >= 0 && entry.running && !list[at].running) return list;
  if (at >= 0) return [...list.slice(0, at), entry, ...list.slice(at + 1)];
  let i = list.length;
  while (i > 0 && list[i - 1].time > entry.time) i--;
  return [...list.slice(0, i), entry, ...list.slice(i)];
}

/** `list` kept as each log keeps it: the newest {@link CONSOLE_KEEP} entries of each log `logOf`
 *  tells apart, of them at most {@link CONSOLE_KEEP_READS} reads. What still runs always stays. */
export function trimmed<T extends ConsoleEntry>(list: T[], logOf: (entry: T) => string | null): T[] {
  const entries = new Map<string | null, number>();
  const reads = new Map<string | null, number>();
  const drop = new Set<T>();
  for (let i = list.length - 1; i >= 0; i--) {
    const entry = list[i];
    if (entry.running) continue;
    const log = logOf(entry);
    if (entry.kind === "read") {
      const n = (reads.get(log) ?? 0) + 1;
      reads.set(log, n);
      if (n > CONSOLE_KEEP_READS) {
        drop.add(entry);
        continue;
      }
    }
    const n = (entries.get(log) ?? 0) + 1;
    entries.set(log, n);
    if (n > CONSOLE_KEEP) drop.add(entry);
  }
  return drop.size ? list.filter((e) => !drop.has(e)) : list;
}

/** Of entries that came for logs while a load was on its way, the ones still to show once
 *  `cleared` were cleared: what still runs. A done one was in the log, and is gone with it. */
export function afterClear<T extends { log: string | null; entry: ConsoleEntry }>(pending: T[], cleared: (string | null)[]): T[] {
  const logs = new Set(cleared);
  return pending.filter((p) => p.entry.running || !logs.has(p.log));
}

/** Lines of output shown at once under an action's own command; more fold under "N lines of output". */
export const OPEN_LINES = 8;

/** Whether an own command's output shows at once: it went wrong, or it is short and of an action. */
export function outputOpen(entry: ConsoleEntry, ran: ConsoleRan): boolean {
  const lines = ran.output?.length ?? 0;
  if (!lines) return false;
  if (failed(ran) || ran.running) return true;
  return entry.kind === "action" && lines <= OPEN_LINES;
}
