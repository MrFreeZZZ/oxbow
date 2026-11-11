// Compare keeps full Git revisions in its query and short names in its controls.
const prefixes = {
  local: "refs/heads/",
  remote: "refs/remotes/",
  tag: "refs/tags/",
} as const;

export function compareRef(kind: keyof typeof prefixes, name: string): string {
  return prefixes[kind] + name;
}

export const isCompareSha = (rev: string): boolean => /^[0-9a-f]{4,64}$/i.test(rev);

export function compareLabel(rev: string): string {
  for (const prefix of Object.values(prefixes)) {
    if (rev.startsWith(prefix)) return rev.slice(prefix.length);
  }
  return /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/i.test(rev) ? rev.slice(0, 7) : rev;
}

export function compareHead(branch: string | null): string {
  return branch === null ? "HEAD" : compareRef("local", branch);
}

export function quoteGitArg(arg: string): string {
  return /^[\w./:=@+,%^-]+$/.test(arg) ? arg : "'" + arg.replace(/'/g, "'\\''") + "'";
}
