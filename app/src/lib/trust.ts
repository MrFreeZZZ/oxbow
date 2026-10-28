// Repository trust in words: what each setting that runs a program does, and what Restricted
// gives git in its place.

import type { TrustFinding } from "./types";

const HOOKS: Record<string, string> = {
  "pre-commit": "Runs before each commit",
  "prepare-commit-msg": "Runs on each commit message",
  "commit-msg": "Runs on each commit message",
  "post-commit": "Runs after each commit",
  "pre-merge-commit": "Runs before each merge commit",
  "post-checkout": "Runs after switching branches",
  "post-merge": "Runs after each merge and pull",
  "pre-push": "Runs before each push",
  "pre-rebase": "Runs before each rebase",
  "post-rewrite": "Runs after amending and rebasing",
  "reference-transaction": "Runs on every change to a branch or tag",
  "fsmonitor-watchman": "Runs every time Oxbow reads the changes",
  "post-index-change": "Runs every time the staged files change",
  "pre-auto-gc": "Runs before git tidies up the repository",
  "applypatch-msg": "Runs when applying patches",
  "pre-applypatch": "Runs when applying patches",
  "post-applypatch": "Runs when applying patches",
};

/** What the setting makes git run, and when. */
export function trustWhy(f: TrustFinding): string {
  const key = f.key.toLowerCase();
  switch (f.kind) {
    case "fsmonitor":
      return "Runs every time Oxbow reads the changes";
    case "hook":
      if (key === "core.hookspath") return "Hooks run from this folder";
      return HOOKS[f.key.slice("hooks/".length)] ?? "Runs when git gets to that step";
    case "ssh":
      return "Replaces ssh for fetch and push";
    case "credential":
      return "Gets every password and token git asks for";
    case "filter":
      return "Runs on every file marked with this filter";
    case "textconv":
      return "Runs on every file shown with this diff driver";
    case "mergeDriver":
      return "Runs on every file merged with this driver";
    case "mergeTool":
      return "Runs when you open the merge tool";
    case "signing":
      return "Runs on every signed commit and tag";
    case "remote":
      return "Runs when fetching from or pushing to this remote";
    case "command":
      if (key === "core.askpass") return "Asks for your passwords";
      if (key === "core.gitproxy") return "Connects to git:// servers";
      if (key === "sequence.editor") return "Edits the steps of a rebase";
      if (key.startsWith("submodule.")) return "Runs when updating this submodule";
      if (key.startsWith("protocol.")) return "Lets a remote address run commands (ext::)";
      if (key.startsWith("lfs.")) return "Moves Git LFS files";
      return "Runs when fetching";
  }
}

/** What git does in its place while the repository is restricted. */
export function restrictedAs(f: TrustFinding): string {
  switch (f.kind) {
    case "ssh":
      return "your ssh";
    case "credential":
      return "your helpers";
    case "signing":
      return "your program";
    case "remote":
      return "git’s own";
    default:
      return "off";
  }
}

/** The popover's lines: hooks together as one, the rest each by its setting. */
export function restrictedLines(found: TrustFinding[]): { label: string; as: string }[] {
  const hooks = found.filter((f) => f.kind === "hook" && f.key.startsWith("hooks/")).length;
  const lines = found.filter((f) => !(f.kind === "hook" && f.key.startsWith("hooks/"))).map((f) => ({ label: f.key, as: restrictedAs(f) }));
  if (hooks) lines.push({ label: `Hooks (${hooks})`, as: "off" });
  return lines;
}

/** A trusted repository's settings, short, for Settings: hooks as one word, two at most. */
export function settingsTag(settings: string[]): string {
  const names = [...new Set(settings.map((s) => (s.startsWith("hooks/") ? "hooks" : s)))];
  if (names.length <= 2) return names.join(", ");
  return `${names.slice(0, 2).join(", ")} +${names.length - 2}`;
}
