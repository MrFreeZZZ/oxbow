// Formatting helpers and the branch palette.

/** CSS variable for a lane color: 0 is the trunk (Steel), 1..8 the branch palette. */
export const lane = (color: number) => `var(--lane-${color})`;
/** Darker (light theme) or brighter (dark theme) text color for labels on a lane tint. */
export const plate = (color: number) => `var(--plate-${color})`;
/** A soft tint of a lane color, for selection and label fills. */
export const tint = (color: number, strength: "soft" | "label" | "bar" = "soft") =>
  `color-mix(in srgb, var(--lane-${color}) var(--tint-${strength}), transparent)`;

export const shortId = (id: string) => id.slice(0, 7);

const minute = 60;
const hour = 60 * minute;
const day = 24 * hour;

const timeOfDay = (date: Date) =>
  date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });

/** "2 min ago", "3 hr ago", "Yesterday 17:05", "Mon 09:12", "12 Mar", "12 Mar 2023". */
export function relativeTime(seconds: number, now = Date.now() / 1000): string {
  const diff = Math.max(0, now - seconds);
  if (diff < minute) return "Just now";
  if (diff < hour) return `${Math.floor(diff / minute)} min ago`;
  const date = new Date(seconds * 1000);
  const today = new Date(now * 1000);
  const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();
  if (sameDay(date, today)) return `${Math.floor(diff / hour)} hr ago`;
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (sameDay(date, yesterday)) return `Yesterday ${timeOfDay(date)}`;
  if (diff < 6 * day) return `${date.toLocaleDateString(undefined, { weekday: "short" })} ${timeOfDay(date)}`;
  const sameYear = date.getFullYear() === today.getFullYear();
  return date.toLocaleDateString(undefined, sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" });
}

/** Full date for the commit panel, in the commit's own time zone offset. */
export function fullDate(seconds: number, offset: number): string {
  const local = new Date((seconds + offset) * 1000);
  const sign = offset < 0 ? "-" : "+";
  const abs = Math.abs(offset);
  const zone = `${sign}${String(Math.floor(abs / 3600)).padStart(2, "0")}${String(Math.floor((abs % 3600) / 60)).padStart(2, "0")}`;
  const date = local.toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
  const time = local.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", timeZone: "UTC" });
  return `${date} at ${time} (${zone})`;
}

/** Stable palette color for a person, from their email. */
export function personColor(email: string): number {
  let hash = 0x811c9dc5;
  for (const ch of email.toLowerCase()) {
    hash ^= ch.charCodeAt(0);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return 1 + (hash % 9);
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  return (parts[0][0] + (parts.length > 1 ? parts[parts.length - 1][0] : "")).toUpperCase();
}

/** Split a path into its folder (with trailing slash) and file name. */
export function splitPath(path: string): { dir: string; name: string } {
  const i = path.lastIndexOf("/");
  return i < 0 ? { dir: "", name: path } : { dir: path.slice(0, i + 1), name: path.slice(i + 1) };
}

/**
 * Short labels for file tabs: the file name, and for files that share a name, as many parent
 * folders as it takes to tell them apart, like PyCharm: "email/README.md", "resetpwd/README.md".
 */
export function tabLabels(paths: string[]): Map<string, string> {
  const parts = new Map(paths.map((p) => [p, p.split("/")]));
  const label = (p: string, depth: number) => parts.get(p)!.slice(-depth).join("/");
  const depth = new Map(paths.map((p) => [p, 1]));
  // Deepen every label that still collides until all are unique or show the whole path.
  for (;;) {
    const seen = new Map<string, string[]>();
    for (const p of paths) {
      const l = label(p, depth.get(p)!);
      seen.set(l, [...(seen.get(l) ?? []), p]);
    }
    let grew = false;
    for (const group of seen.values()) {
      if (group.length < 2) continue;
      for (const p of group) {
        if (depth.get(p)! < parts.get(p)!.length) {
          depth.set(p, depth.get(p)! + 1);
          grew = true;
        }
      }
    }
    if (!grew) break;
  }
  return new Map(paths.map((p) => [p, label(p, depth.get(p)!)]));
}
