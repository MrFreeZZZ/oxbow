// What the Settings window reads besides Oxbow's own settings: Git's settings from `git config
// --list` (whose keys are lower case), this repository's, the GitHub and GitLab accounts and the
// trusted repositories, with the rows that show and write them.

import { api } from "./api";
import { prefs, type PrefKey } from "./prefs.svelte";
import type { Option, Row, Setting, Text, Value } from "./settingsRows";
import type { ConfigScope, GitHubAccount, GitLabAccount, GitSettings, RepoSettings, TrustedRepo } from "./types";

export const model = $state({
  /** The window's search, over every section. */
  query: "",
  git: null as GitSettings | null,
  repo: null as RepoSettings | null,
  /** Who is signed in to GitHub. */
  account: null as GitHubAccount | null,
  /** Who is signed in to GitLab: one account per server. */
  gitlab: [] as GitLabAccount[],
  /** Repositories trusted to run their own settings' programs, with such settings. */
  trusted: [] as TrustedRepo[],
  /** Why the last write to Git's settings failed. */
  problem: null as string | null,
  /** Monospaced font families installed here, for the Font popup. */
  fonts: [] as string[],
  /** What a Copy button just copied, to say Copied for a moment. */
  copied: null as string | null,
  /** The GitHub sign-in sheet. */
  signIn: null as { tokenFirst: boolean } | null,
  /** The GitLab sign-in sheet, with the server to start from. */
  gitlabSignIn: null as { server: string } | null,
});

export async function load() {
  const [g, r, a, l, t] = await Promise.all([
    api.gitSettings().catch(() => null),
    api.repoSettings().catch(() => null),
    api.githubAccount().catch(() => null),
    api.gitlabAccounts().catch(() => []),
    api.trustedRepos().catch(() => []),
  ]);
  model.git = g;
  model.repo = r;
  model.account = a;
  model.gitlab = l;
  model.trusted = t;
}

export function copy(id: string, value: string) {
  navigator.clipboard.writeText(value).then(() => {
    model.copied = id;
    setTimeout(() => model.copied === id && (model.copied = null), 1500);
  });
}

// Oxbow's own settings.
export const prefRow = (key: PrefKey, label: Text, control: "seg" | "popup" | "switch", more: Partial<Row> & { options?: () => Option[] } = {}): Row => ({
  id: key,
  label,
  setting: { kind: "pref", key },
  control:
    control === "switch"
      ? { type: "switch", get: () => !!prefs.get(key), set: (on) => prefs.set(key, on as never) }
      : { type: control, options: more.options ?? (() => []), get: () => prefs.get(key) as Value, set: (value) => prefs.set(key, value as never) },
  ...more,
});

// Git's own settings.
export function config(scope: ConfigScope, key: string): string | undefined {
  const map = scope === "global" ? model.git?.global : model.repo?.local;
  return map?.[key.toLowerCase()];
}

export const truthy = (value: string | undefined) => value !== undefined && !["false", "no", "off", "0", ""].includes(value.toLowerCase());

/** Write Git settings in order; `null` removes a value. */
export async function writeGit(scope: ConfigScope, values: [string, string | null][]) {
  model.problem = null;
  const map = scope === "global" ? model.git?.global : model.repo?.local;
  for (const [key, value] of values) {
    try {
      await api.setGitConfig(scope, key, value);
      if (map) {
        if (value === null) delete map[key.toLowerCase()];
        else map[key.toLowerCase()] = value;
      }
    } catch (err) {
      model.problem = String(err);
      break;
    }
  }
  load();
}

const flag = (scope: ConfigScope) => (scope === "global" ? "--global" : "--local");
const quote = (value: string) => (/^[\w@%+=:,./~-]+$/.test(value) ? value : `"${value.replace(/(["\\$`])/g, "\\$1")}"`);
const configLine = (scope: ConfigScope, key: string, value: string | null) =>
  value === null ? `git config ${flag(scope)} --unset ${key}` : `git config ${flag(scope)} ${key} ${quote(value)}`;

export function gitSetting(scope: ConfigScope, writes: () => [string, string | null][], unset: string[]): Setting {
  return {
    kind: "git",
    scope,
    keys: unset,
    commands: () => writes().map(([key, value]) => configLine(scope, key, value)),
    reset: () => writeGit(scope, unset.map((key) => [key, null])),
  };
}

/** A Git switch; `fallback` is what Oxbow does while the key is not set. */
export function gitSwitch(scope: ConfigScope, key: string | string[], label: Text, fallback: boolean, more: Partial<Row> = {}): Row {
  const keys = Array.isArray(key) ? key : [key];
  const get = () => {
    const value = config(scope, keys[0]);
    return value === undefined ? fallback : truthy(value);
  };
  return {
    id: `git:${keys[0]}`,
    label,
    control: { type: "switch", get, set: (on) => writeGit(scope, keys.map((k) => [k, String(on)])) },
    setting: gitSetting(scope, () => keys.map((k) => [k, String(get())]), keys),
    ...more,
  };
}

/** A Git text value with Edit. */
export function gitEdit(scope: ConfigScope, key: string, label: Text, more: Partial<Row> & { shown?: () => string; placeholder?: string } = {}): Row {
  const get = () => config(scope, key) ?? "";
  return {
    id: `git:${scope}:${key}`,
    label,
    control: { type: "edit", get, shown: more.shown, placeholder: more.placeholder, save: (value) => writeGit(scope, [[key, value.trim() || null]]) },
    setting: gitSetting(scope, () => [[key, get() || null]], [key]),
    ...more,
  };
}
