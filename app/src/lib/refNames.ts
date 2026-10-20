// Long branch names in the sidebar: many share a start ("claude/project-thread-…"), and cut at
// the end they all look alike. The start a name shares with another one in the same list gives
// way first, so the part that tells them apart stays in sight.

/** Characters a name shares with its nearest neighbor in the list, cut back to just after a
 *  `/`, `-`, `_` or `.`, so the part kept starts a word. 0 when it shares nothing worth hiding. */
export function sharedLeads(names: string[]): Map<string, number> {
  const sorted = [...new Set(names)].sort();
  const common = (a: string, b: string) => {
    let i = 0;
    while (i < a.length && i < b.length && a[i] === b[i]) i++;
    return i;
  };
  const out = new Map<string, number>();
  sorted.forEach((name, i) => {
    let n = Math.max(i > 0 ? common(name, sorted[i - 1]) : 0, i < sorted.length - 1 ? common(name, sorted[i + 1]) : 0);
    // Never the whole name: something must be left to tell it apart.
    n = Math.min(n, name.length - 1);
    while (n > 0 && !"/-_.".includes(name[n - 1])) n--;
    // A short shared start is no help hidden.
    out.set(name, n >= 6 ? n : 0);
  });
  return out;
}
