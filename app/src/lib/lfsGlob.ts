// Matching .gitattributes patterns, as git does, without building a regular expression from
// them: a repository's patterns could make one backtrack for ages, or not compile.

/** Whether a .gitattributes pattern matches `path`: without a slash it matches a name in any
 *  folder, with one a path from the top. */
export function matches(pattern: string, path: string): boolean {
  const anchored = pattern.replace(/\/$/, "").includes("/");
  const target = anchored ? path : path.slice(path.lastIndexOf("/") + 1);
  return segmentsMatch(pattern.replace(/^\//, "").split("/"), target.split("/"));
}

/** `**` is any folders; a trailing one everything inside. A table of pattern by path position,
 *  so many `**` never try every way to split the path. */
function segmentsMatch(pattern: string[], path: string[]): boolean {
  let reached = path.map(() => false).concat(false);
  reached[0] = true;
  pattern.forEach((part, n) => {
    const next = path.map(() => false).concat(false);
    if (part === "**") {
      const first = reached.indexOf(true);
      // A trailing `**` is what is inside the folder, not the folder itself.
      const from = n === pattern.length - 1 ? first + 1 : first;
      if (first >= 0) for (let i = from; i <= path.length; i++) next[i] = true;
    } else {
      for (let i = 0; i < path.length; i++) next[i + 1] = reached[i] && nameMatches(part, path[i]);
    }
    reached = next;
  });
  return reached[path.length];
}

/** `*`, `?` and `[…]` within one name: a `*` goes back at most to the last one, so it stays
 *  quick whatever the pattern. */
function nameMatches(pattern: string, name: string): boolean {
  let p = 0;
  let t = 0;
  let star = -1;
  let mark = 0;
  while (t < name.length) {
    const step = p < pattern.length ? matchOne(pattern, p, name[t]) : null;
    if (step !== null && pattern[p] !== "*") {
      p = step;
      t++;
    } else if (p < pattern.length && pattern[p] === "*") {
      while (pattern[p] === "*") p++;
      star = p;
      mark = t;
    } else if (star >= 0) {
      p = star;
      t = ++mark;
    } else return false;
  }
  while (pattern[p] === "*") p++;
  return p === pattern.length;
}

/** Where the pattern goes on when its part at `p` matches the character `c`; `null` when it
 *  doesn't. A `[` without its `]` is just a `[`. */
function matchOne(pattern: string, p: number, c: string): number | null {
  const ch = pattern[p];
  if (ch === "?") return p + 1;
  if (ch === "\\" && p + 1 < pattern.length) return pattern[p + 1] === c ? p + 2 : null;
  if (ch === "[") {
    const end = pattern.indexOf("]", p + 2);
    if (end > 0) {
      let i = p + 1;
      const negated = pattern[i] === "!" || pattern[i] === "^";
      if (negated) i++;
      let found = false;
      for (; i < end; i++) {
        if (pattern[i + 1] === "-" && i + 2 < end) {
          if (pattern[i] <= c && c <= pattern[i + 2]) found = true;
          i += 2;
        } else if (pattern[i] === c) found = true;
      }
      return found !== negated ? end + 1 : null;
    }
  }
  return ch === c ? p + 1 : null;
}
