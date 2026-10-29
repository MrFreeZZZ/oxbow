// The Settings window's sections that write Git's own settings with `git config`, so a terminal
// sees the same: Accounts, Git and This Repository, with the GitHub sign-in, the secrets check's
// own patterns and the trusted repositories.

import { open as openFile } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { confirm, type Request } from "./confirm.svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { prefs, type SecretPattern } from "./prefs.svelte";
import { addRemoteRequest, optimizeRequest, removeRemoteRequest, setUrlRequest } from "./repoSettings";
import { config, copy, gitEdit, gitSetting, gitSwitch, load, model, prefRow, truthy, writeGit } from "./settingsModel.svelte";
import type { Row, Section, Value } from "./settingsRows";
import { tokenExpiry } from "./gitlab";
import { settingsTag } from "./trust";
import { checkCommands, checkProblem, checkSub, movedCheck, withCheck } from "./commitChecks";
import { matching, PROTECTED_KEY, protectedProblem, protectedValue } from "./protect";
import type { CommitCheck, GitLabAccount, RemoteInfo } from "./types";

// GitHub: who is signed in, and the sign-in sheet.
const storeName = navigator.platform.startsWith("Mac") ? "Keychain" : navigator.platform.startsWith("Win") ? "Credential Manager" : "keyring";
const lacksRepo = () => !!model.account?.scopes && !model.account.scopes.includes("repo");

/** Where the token is kept, as the command that would remove it. */
function forgetCommand(login: string): string {
  const platform = navigator.platform;
  if (platform.startsWith("Mac")) return `security delete-generic-password -s "Oxbow GitHub" -a ${login}`;
  if (platform.startsWith("Win")) return `cmdkey /delete:${login}.Oxbow GitHub`;
  return `secret-tool clear service "Oxbow GitHub" username ${login}`;
}

async function signOut() {
  if (!model.account) return;
  const login = model.account.login;
  await confirm.run({
    title: `Sign out of GitHub as ${login}?`,
    body: [
      `Oxbow forgets the token: it leaves the ${storeName}, and Publish and`,
      " pull requests need a new sign-in. Git’s own saved passwords and SSH keys stay. GitHub keeps the authorization until you revoke it in ",
      { code: "Settings › Applications" },
      " there.",
    ],
    icon: "key",
    button: "Sign Out",
    status: "Signing out…",
    local: {
      commands: [forgetCommand(login)],
      comment: "Oxbow does this itself; no git command runs",
      run: async () => {
        await api.githubSignOut();
        await load();
        return `Signed out of GitHub.`;
      },
    },
  });
}

/** The command that removes a GitLab token from the credential store. */
function forgetGitLabCommand(item: string): string {
  const platform = navigator.platform;
  if (platform.startsWith("Mac")) return `security delete-generic-password -s "Oxbow GitLab" -a '${item}'`;
  if (platform.startsWith("Win")) return `cmdkey /delete:'${item}.Oxbow GitLab'`;
  return `secret-tool clear service "Oxbow GitLab" username '${item}'`;
}

async function gitlabSignOut(account: GitLabAccount) {
  await confirm.run({
    title: `Sign out of ${account.host} as ${account.username}?`,
    body: [
      `Oxbow forgets the token: it leaves the ${storeName}, and merge requests there go back to working through the push. Git’s own saved passwords and SSH keys stay. GitLab keeps the token until it expires or you revoke it in `,
      { code: "Preferences › Access tokens" },
      " there.",
    ],
    icon: "key",
    button: "Sign Out",
    status: "Signing out…",
    local: {
      commands: [forgetGitLabCommand(account.item)],
      comment: "Oxbow does this itself; no git command runs",
      run: async () => {
        await api.gitlabSignOut(account.host);
        await load();
        return `Signed out of ${account.host}.`;
      },
    },
  });
}

// Pull: rebase unless pull.rebase is false (merge) or pull.ff is only.
const pullMode = () => (config("global", "pull.ff") === "only" ? "ff" : config("global", "pull.rebase") !== undefined && !truthy(config("global", "pull.rebase")) ? "merge" : "rebase");
const pullWrites = (mode: string): [string, string | null][] =>
  mode === "ff" ? [["pull.ff", "only"], ["pull.rebase", null]] : mode === "merge" ? [["pull.ff", null], ["pull.rebase", "false"]] : [["pull.ff", null], ["pull.rebase", "true"]];

// Signing: SSH signing with a key from ~/.ssh.
const signingKey = () => config("global", "user.signingkey");
const sshFormat = () => config("global", "gpg.format") === "ssh";
const firstKey = () => model.git?.sshKeys.find((k) => k.name === "id_ed25519") ?? model.git?.sshKeys[0] ?? null;
const keyName = (path: string | undefined) => model.git?.sshKeys.find((k) => k.path === path)?.name ?? path?.split("/").pop() ?? "";

function setSigning(on: boolean) {
  if (!on) return writeGit("global", [["commit.gpgsign", "false"]]);
  const writes: [string, string | null][] = [];
  if (!signingKey() && firstKey()) writes.push(["gpg.format", "ssh"], ["user.signingkey", firstKey()!.path]);
  writes.push(["commit.gpgsign", "true"]);
  return writeGit("global", writes);
}

// This repository's own identity: on while its .git/config names an author.
const ownIdentity = () => config("local", "user.name") !== undefined || config("local", "user.email") !== undefined;
const who = () => [config("global", "user.name"), config("global", "user.email")].filter(Boolean).join(" · ") || "nobody yet: set a name in Accounts";

async function chooseGit() {
  const path = await openFile({ multiple: false, directory: false, title: "Choose the git to run" });
  if (typeof path !== "string") return;
  await prefs.set("oxbow.git.path", path);
  load();
}

async function untrust(path: string) {
  await api.untrustRepo(path);
  model.trusted = model.trusted.filter((t) => t.path !== path);
}

function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(bytes < 10 * 1024 ** 2 ? 1 : 0)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

async function runRepo(request: Parameters<typeof confirm.run>[0]) {
  await confirm.run(request);
  load();
}

/** Write the repository's checks before commit whole, then read Settings again. */
async function saveChecks(list: CommitCheck[]) {
  model.problem = null;
  try {
    await api.setCommitChecks(list);
  } catch (err) {
    model.problem = String(err);
  }
  await load();
}

/** The sheet that adds a check before commit (`index` null) or edits one. */
function checkRequest(index: number | null, command: string): Request {
  const list = model.repo?.checks ?? [];
  const invalid = checkProblem(list, index, command);
  const next = withCheck(list, index, command);
  return {
    title: index === null ? "Add a check before commit" : "Edit the check",
    body: [
      "Oxbow runs it in ",
      { code: model.repo?.name ?? "this repository" },
      " before each commit, after the secrets check. When it fails, nothing is committed: its output is in the sheet and the Console, with Commit Anyway.",
    ],
    icon: "hook",
    button: index === null ? "Add Check" : "Save Check",
    always: true,
    invalid,
    fields: [
      {
        label: "Command",
        text: { value: command, placeholder: "ruff check .", focus: true, edit: (value) => checkRequest(index, value) },
        note: "Runs in your login shell from the repository’s top folder, as in a terminal, e.g. npm test, cargo fmt --check or ./scripts/lint.sh. An exit code other than 0 is a failure.",
        error: command.trim() && invalid ? invalid : undefined,
      },
    ],
    status: "Saving the check…",
    local: {
      commands: checkCommands(next),
      run: async () => {
        await saveChecks(next);
        if (model.problem) throw new Error(model.problem);
        return index === null ? `Added ${command.trim()}.` : `Saved ${command.trim()}.`;
      },
    },
  };
}

/** Keep `list` as the protected branches; `null` goes back to the default list. */
async function saveProtected(list: string[] | null) {
  await writeGit("local", [[PROTECTED_KEY, list === null ? null : protectedValue(list)]]);
}

/** "Matches main and origin/main", from the branches Settings knows. */
function matchNote(pattern: string): string {
  const found = matching(pattern, model.repo?.production.branches ?? []);
  if (!found.length) return pattern.includes("*") ? "No branch here matches it yet" : "No branch of this name here yet";
  const shown = found.slice(0, 3).join(", ");
  return found.length > 3 ? `Matches ${shown} and ${found.length - 3} more` : `Matches ${shown}`;
}

/** The sheet that protects one more branch or pattern. */
function protectRequest(name: string): Request {
  const list = model.repo?.protected.branches ?? [];
  const invalid = protectedProblem(list, name);
  const next = [...list, name.trim()];
  return {
    title: "Protect a branch",
    body: [
      "Oxbow won’t force push it in ",
      { code: model.repo?.name ?? "this repository" },
      " or rewrite its commits that are already pushed; Revert undoes them instead.",
    ],
    icon: "branch",
    button: "Protect",
    always: true,
    invalid,
    fields: [
      {
        label: "Branch",
        text: { value: name, placeholder: "release/*", focus: true, edit: (value) => protectRequest(value) },
        note: name.trim() && !invalid ? matchNote(name.trim()) : "A branch name, or a pattern where * stands for any characters, e.g. release/* or *-stable.",
        error: name.trim() && invalid ? invalid : undefined,
      },
    ],
    status: "Saving…",
    local: {
      commands: [`git config --local ${PROTECTED_KEY} "${protectedValue(next)}"`],
      run: async () => {
        await saveProtected(next);
        if (model.problem) throw new Error(model.problem);
        return `${name.trim()} is protected.`;
      },
    },
  };
}

/** The protected branches, the row that adds one, and the way back to the default list. The
 *  default list shows only the names this repository has a branch of; the others stay protected. */
function protectedRows(protect: { branches: string[]; default: boolean }): Row[] {
  const branches = model.repo?.production.branches ?? [];
  const shown = protect.branches.flatMap((name, i) => (!protect.default || matching(name, branches).length ? [{ name, i }] : []));
  const rows: Row[] = shown.map(({ name, i }) => ({
    id: `protected:${name}`,
    label: name,
    mono: true,
    sub: () => matchNote(name),
    badge: () => (protect.default ? "default" : null),
    control: { type: "button", label: "Remove", run: () => saveProtected(protect.branches.filter((_, j) => j !== i)) },
  }));
  rows.push({
    id: "protected:add",
    label: shown.length ? "Protect another branch" : protect.branches.length ? "Protect a branch" : "No protected branches",
    sub: protect.branches.length ? undefined : "Every branch can be force pushed and rewritten",
    control: { type: "button", label: "Add…", run: () => confirm.run(protectRequest("")) },
  });
  if (!protect.default) {
    rows.push({
      id: "protected:default",
      label: "Default list",
      sub: "main, master and the production branch",
      control: { type: "button", label: "Use", run: () => saveProtected(null) },
    });
  }
  return rows;
}

/** The rows of the checks before commit, and the one that adds a check. */
function checkRows(checks: CommitCheck[], restricted: boolean): Row[] {
  const rows = checks.map(
    (check, i): Row => ({
      id: `check:${i}`,
      label: check.command,
      mono: true,
      sub: () => checkSub(check, restricted),
      subTone: () => (restricted ? "warn" : undefined),
      control: { type: "switch", get: () => check.on, set: (on) => saveChecks(checks.map((c, j) => (j === i ? { ...c, on } : { ...c }))) },
      more: () => {
        const entries: MenuEntry[] = [{ kind: "item", label: "Edit…", icon: menuIcons.edit, run: () => confirm.run(checkRequest(i, check.command)) }];
        if (i > 0) entries.push({ kind: "item", label: "Run Earlier", icon: menuIcons.up, run: () => saveChecks(movedCheck(checks, i, -1)) });
        if (i < checks.length - 1) entries.push({ kind: "item", label: "Run Later", icon: menuIcons.down, run: () => saveChecks(movedCheck(checks, i, 1)) });
        entries.push({ kind: "sep" }, { kind: "item", label: "Remove Check", icon: menuIcons.drop, danger: true, run: () => saveChecks(checks.filter((_, j) => j !== i)) });
        return entries;
      },
    }),
  );
  rows.push({
    id: "check:add",
    label: checks.length ? "Add a check" : "No checks before commit yet",
    sub: checks.length ? undefined : "e.g. ruff check ., npm test or cargo fmt --check",
    control: { type: "button", label: "Add…", run: () => confirm.run(checkRequest(null, "")) },
  });
  return rows;
}

/** The sheet that adds a secret pattern of your own. */
function patternRequest(pattern: string, kind: SecretPattern["kind"]): Request {
  const list = prefs.get("oxbow.secrets.patterns");
  const text = pattern.trim();
  let invalid: string | null = text ? null : "Type a pattern";
  if (text && kind === "text") {
    try {
      new RegExp(text);
    } catch {
      invalid = "This is not a regular expression";
    }
  }
  if (text && list.some((p) => p.kind === kind && p.pattern === text)) invalid = "This pattern is in the list already";
  return {
    title: "Add a secret pattern",
    body: ["Oxbow checks staged lines for it before each commit, in every repository, together with the built-in patterns."],
    icon: "key",
    button: "Add Pattern",
    always: true,
    invalid: text ? invalid : "Type a pattern",
    fields: [
      {
        label: "Looks for",
        chips: [
          { label: "Variable name", on: kind === "name", pick: () => patternRequest(pattern, "name") },
          { label: "Text (regex)", on: kind === "text", pick: () => patternRequest(pattern, "text") },
        ],
      },
      {
        label: "Pattern",
        text: { value: pattern, placeholder: kind === "name" ? "CLIENT_ID or PARK_*" : "pk_live_[0-9a-zA-Z]{24}", focus: true, edit: (value) => patternRequest(value, kind) },
        note: kind === "name" ? `Flags lines that give it a value: NAME = "…", NAME: … or "NAME": "…". * stands for any characters; case does not matter.` : "Flags any added line where this matches, e.g. a key with a known prefix.",
        error: text && invalid ? invalid : undefined,
      },
    ],
    status: "Adding the pattern…",
    local: {
      commands: [],
      run: async () => {
        await prefs.save("oxbow.secrets.patterns", [...list, { kind, pattern: text }]);
        return `Added ${text}.`;
      },
    },
  };
}

function removePattern(index: number) {
  const list = prefs.get("oxbow.secrets.patterns");
  prefs.set("oxbow.secrets.patterns", list.filter((_, i) => i !== index));
}

/** Turning the secrets check off always asks, even with confirmations off. */
async function checkSecretsOff() {
  await confirm.run({
    title: "Turn off the secrets check?",
    body: [
      "Commits in every repository go through without Oxbow looking for passwords, tokens and private keys in the staged lines. A secret that reaches a remote has to be revoked: deleting it from history is not enough.",
    ],
    icon: "key",
    button: "Turn Off",
    danger: true,
    always: true,
    status: "Turning the secrets check off…",
    local: {
      commands: [],
      run: async () => {
        await prefs.save("oxbow.commit.checkSecrets", false);
        return "The secrets check is off.";
      },
    },
  });
}

export const accountsSection: Section = {
  id: "accounts",
  label: "Accounts",
  color: "var(--lane-0)",
  icon: "M8 8a2.6 2.6 0 1 0 0-5.2a2.6 2.6 0 1 0 0 5.2M3 13.8c.6-2.6 2.6-3.6 5-3.6s4.4 1 5 3.6",
  groups: () => [
    {
      title: "GitHub",
      foot: "Git keeps using your SSH keys and saved passwords. Oxbow lends git this sign-in for https://github.com remotes that have none.",
      rows: model.account
        ? [
            {
              id: "github:account",
              label: model.account.name ?? model.account.login,
              image: () => model.account?.avatarUrl ?? null,
              sub: () => `${model.account?.name ? `@${model.account.login} · ` : ""}${model.account?.method === "browser" ? "signed in with the browser" : "signed in with a token"} · the token is in the ${storeName}`,
              badge: () => "Connected",
              control: { type: "button", label: "Sign Out…", run: signOut },
              more: () => [{ kind: "item", label: "Open Profile on GitHub", icon: menuIcons.reveal, run: () => model.account && api.openGitHub(model.account.htmlUrl) }],
            },
            ...(lacksRepo()
              ? [
                  {
                    id: "github:scopes",
                    label: "This token can’t publish or open pull requests",
                    sub: () => `It needs the repo scope; it has ${model.account?.scopes?.length ? model.account.scopes.join(", ") : "none"}.`,
                    control: { type: "button" as const, label: "Sign In Again…", run: () => (model.signIn = { tokenFirst: model.account?.method === "token" }) },
                  },
                ]
              : []),
          ]
        : [
            {
              id: "github:none",
              label: "GitHub",
              sub: "Sign in to publish repositories and work with pull requests",
              control: { type: "button", label: "Sign In…", run: () => (model.signIn = { tokenFirst: false }) },
            },
          ],
    },
    {
      title: "GitLab",
      foot: "Without signing in, Create Merge Request works through the push. Signing in to gitlab.com or a server of your own (GitLab 16.0 or later) brings merge requests into Oxbow.",
      rows: [
        ...model.gitlab.map((account): Row => {
          const expiry = tokenExpiry(account.expiresAt);
          return {
            id: `gitlab:${account.host}`,
            label: account.name ?? account.username,
            image: () => account.avatar,
            sub: () => `@${account.username} on ${account.host} · ${expiry?.text ?? "token without an expiry date"}`,
            subTone: () => (expiry?.expired ? "error" : expiry?.soon ? "warn" : undefined),
            badge: () => (expiry?.expired ? "Expired" : expiry?.soon ? "Expires soon" : "Connected"),
            control: expiry?.soon
              ? { type: "button", label: "Sign In Again…", run: () => (model.gitlabSignIn = { server: account.web }) }
              : { type: "button", label: "Sign Out…", run: () => gitlabSignOut(account) },
            more: () => [
              { kind: "item", label: "Open Profile on GitLab", icon: menuIcons.reveal, run: () => void api.gitlabOpenProfile(account.host) },
              { kind: "item", label: "Sign In Again…", icon: menuIcons.fetch, run: () => void (model.gitlabSignIn = { server: account.web }) },
              { kind: "item", label: "Sign Out…", icon: menuIcons.drop, run: () => void gitlabSignOut(account) },
            ],
          };
        }),
        {
          id: "gitlab:add",
          label: model.gitlab.length ? "Another server" : "GitLab",
          sub: model.gitlab.length ? "gitlab.com or a GitLab server of your own" : "Sign in to work with merge requests on gitlab.com or a server of your own",
          control: { type: "button", label: "Sign In…", run: () => (model.gitlabSignIn = { server: model.gitlab.some((a) => a.host === "gitlab.com") ? "" : "gitlab.com" }) },
        },
      ],
    },
    {
      title: "Commit identity",
      foot: () => `Saved in ~/.gitconfig.${model.repo ? ` A repository can use its own identity, see ${model.repo.name} in the sidebar.` : ""}`,
      rows: [
        gitEdit("global", "user.name", "Name", { shown: () => config("global", "user.name") ?? "Not set", placeholder: "Your Name" }),
        gitEdit("global", "user.email", "Email", { shown: () => config("global", "user.email") ?? "Not set", placeholder: "you@example.com" }),
      ],
    },
    {
      title: "SSH",
      foot: "Add the public key to GitHub or GitLab to fetch and push over SSH.",
      rows: model.git?.sshKeys.length
        ? model.git.sshKeys.map((key) => ({
            id: `ssh:${key.path}`,
            label: key.name,
            mono: true,
            sub: [key.fingerprint, key.comment].filter(Boolean).join(" · "),
            badge: () => (signingKey() === key.path ? "Signs commits" : null),
            control: { type: "button", label: () => (model.copied === key.path ? "Copied" : "Copy Key"), run: () => copy(key.path, key.public) },
          }))
        : [{ id: "ssh:none", label: "No SSH keys in ~/.ssh", sub: "Create one in Terminal with ssh-keygen -t ed25519" }],
    },
    {
      title: "Signing",
      rows: [
        {
          ...gitSwitch("global", "commit.gpgsign", "Sign commits", false),
          control: { type: "switch", get: () => truthy(config("global", "commit.gpgsign")), set: setSigning },
          off: () => !signingKey() && !firstKey(),
          sub: () =>
            truthy(config("global", "commit.gpgsign"))
              ? `Signed with ${sshFormat() ? keyName(signingKey()) : (signingKey() ?? "your GPG key")}. Add it to GitHub as a signing key to show Verified.`
              : !signingKey() && !firstKey()
                ? "Needs an SSH key in ~/.ssh"
                : "Uses your SSH key to sign commits",
        },
        ...(sshFormat() && (model.git?.sshKeys.length ?? 0) > 1
          ? [
              {
                id: "git:user.signingkey",
                label: "Signing key",
                control: {
                  type: "popup" as const,
                  options: () => model.git!.sshKeys.map((k) => ({ value: k.path, label: k.name })),
                  get: () => signingKey() ?? "",
                  set: (value: Value) => writeGit("global", [["user.signingkey", String(value)]]),
                },
                setting: gitSetting("global", () => [["user.signingkey", signingKey() ?? null]], ["user.signingkey"]),
              },
            ]
          : []),
      ],
    },
  ],
};

export const gitSection: Section = {
  id: "git",
  label: "Git",
  color: "var(--section-orange)",
  icon: "M4.5 2v12M4.5 11c0-3 7-2 7-5.5M11.5 5.5a1.6 1.6 0 1 0 0-.1",
  groups: () => [
    {
      title: "Git",
      foot: "Oxbow reads history, graphs and diffs with its built-in engine and calls your Git for everything that changes the repository, so hooks and credentials behave as in Terminal.",
      rows: [
        {
          id: "oxbow.git.path",
          label: () => model.git?.git.path ?? "git",
          mono: true,
          sub: () => (!model.git ? "Looking…" : model.git.git.version ? `git ${model.git.git.version} · commit, rebase, push, hooks` : "This git does not run. Choose another one."),
          setting: { kind: "pref", key: "oxbow.git.path" },
          control: { type: "button", label: "Change…", run: chooseGit },
        },
      ],
    },
    {
      title: "Pull",
      rows: [
        {
          id: "git:pull",
          label: "When pulling",
          control: {
            type: "seg",
            options: () => [
              { value: "merge", label: "Merge" },
              { value: "rebase", label: "Rebase" },
              { value: "ff", label: "Fast-forward" },
            ],
            get: pullMode,
            set: (mode) => writeGit("global", pullWrites(String(mode))),
          },
          setting: gitSetting("global", () => pullWrites(pullMode()), ["pull.rebase", "pull.ff"]),
        },
        gitSwitch("global", ["rebase.autoStash", "merge.autoStash"], "Stash my changes before pulling and bring them back after", true),
        prefRow("oxbow.pull.fetchFirst", "Fetch from the remote before pulling", "switch", {
          sub: () => (prefs.get("oxbow.pull.fetchFirst") ? "Pull runs git fetch first, so every remote branch is current" : "git pull updates only the branch it pulls"),
        }),
        prefRow("oxbow.pull.submodules", "Update submodules after pulling", "switch", {
          sub: () =>
            prefs.get("oxbow.pull.submodules")
              ? "Then git submodule update --init --recursive, so their folders have the commits the pulled commits record"
              : "Submodule folders stay where they are and show as out of sync until updated",
        }),
      ],
    },
    {
      title: "Branches and stacks",
      rows: [
        gitEdit("global", "init.defaultBranch", "Default branch for new repositories", { mono: false, shown: () => config("global", "init.defaultBranch") ?? "master (Git’s default)", placeholder: "main" }),
        gitSwitch("global", "rebase.updateRefs", "Move stacked branches along when rebasing", true, { sub: "git rebase --update-refs: branches built on the rebased one follow it" }),
        gitSwitch("global", "fetch.prune", "Remove branches deleted on the remote when fetching", true),
      ],
    },
    {
      title: "Commits",
      rows: [
        prefRow("oxbow.git.runHooks", "Run Git hooks", "switch", {
          sub: () => (prefs.get("oxbow.git.runHooks") ? "pre-commit, commit-msg and pre-push from .git/hooks" : "Commits, merges and pushes add --no-verify"),
        }),
        prefRow("oxbow.commit.subjectGuide", "Subject line guide", "popup", {
          sub: "A soft marker in the commit message field",
          options: () => [
            { value: 50, label: "50 characters" },
            { value: 72, label: "72 characters" },
            { value: 0, label: "Off" },
          ],
        }),
      ],
    },
    {
      title: "Security",
      foot: "Your patterns are checked together with the built-in set: keys of AWS, GitHub, GitLab, Slack, Stripe, Google and OpenAI, private keys, and names like *API_KEY*, *SECRET*, *TOKEN*, *PASSWORD* that get a value.",
      rows: [
        {
          ...prefRow("oxbow.commit.checkSecrets", "Check for secrets before committing", "switch", {
            sub: () =>
              prefs.get("oxbow.commit.checkSecrets")
                ? "Stops a commit whose staged lines look like a password, token or private key"
                : "Off: commits go through without the check",
          }),
          control: { type: "switch", get: () => prefs.get("oxbow.commit.checkSecrets"), set: (on) => (on ? prefs.set("oxbow.commit.checkSecrets", true) : checkSecretsOff()) },
        },
        ...prefs.get("oxbow.secrets.patterns").map((p, i): Row => ({
          id: `secret:${i}`,
          label: p.pattern,
          mono: true,
          sub: p.kind === "name" ? "Variable name: flags a line that gives it a value" : "Text: a regular expression for the secret itself",
          off: () => !prefs.get("oxbow.commit.checkSecrets"),
          control: { type: "button", label: "Remove", run: () => removePattern(i) },
        })),
        {
          id: "secret:add",
          label: prefs.get("oxbow.secrets.patterns").length ? "Add a pattern" : "No patterns of your own yet",
          sub: prefs.get("oxbow.secrets.patterns").length ? undefined : "e.g. CLIENT_ID, or the API keys of a service you use",
          off: () => !prefs.get("oxbow.commit.checkSecrets"),
          control: { type: "button", label: "Add…", run: () => confirm.run(patternRequest("", "name")) },
        },
      ],
    },
    {
      title: "Repository Trust",
      foot: "Repositories Oxbow cloned or created never ask. Only trusted ones with such settings are listed; removing one makes Oxbow ask again the next time it opens.",
      rows: [
        prefRow("oxbow.git.askTrust", "Ask before opening a repository whose settings run programs", "switch", {
          sub: "Hooks, fsmonitor, filters, ssh and credential commands of the repository itself",
        }),
        ...model.trusted.map((t): Row => ({
          id: `trusted:${t.path}`,
          label: t.path.split(/[/\\]/).filter(Boolean).pop() ?? t.path,
          sub: t.path,
          badge: () => settingsTag(t.settings),
          control: { type: "button", label: "Remove", run: () => untrust(t.path) },
        })),
        ...(model.trusted.length ? [] : [{ id: "trusted:none", label: "No trusted repositories with such settings yet" } as Row]),
      ],
    },
  ],
};

export const repoSection: Section = {
  id: "repo",
  head: "This Repository",
  label: "",
  color: "var(--section-teal)",
  icon: "M1.8 4.5a1 1 0 0 1 1-1H6l1.5 1.5h5.7a1 1 0 0 1 1 1v6.2a1 1 0 0 1-1 1H2.8a1 1 0 0 1-1-1z",
  groups: () => {
    if (!model.repo) return [];
    const r = model.repo;
    const production = r.production;
    const short = (ref: string) => ref.replace(/^refs\/(heads|remotes)\//, "");
    return [
      {
        title: "Graph",
        rows: [
          {
            id: "git:local:oxbow.productionBranch",
            label: "Production branch",
            sub: () =>
              production.chosen && !production.found
                ? `${short(production.chosen)} is not here anymore, so no branch is drawn as production. Choose Auto or another branch.`
                : production.chosen
                  ? "Drawn in the production color and on the thick line"
                  : production.auto
                    ? `Auto picks ${short(production.auto)}: main or master, local first`
                    : "Auto finds no main or master, so no branch is drawn as production",
            control: {
              type: "popup",
              options: () => [
                { value: "", label: production.auto ? `Auto (${short(production.auto)})` : "Auto (main/master)" },
                ...(production.chosen && !production.found ? [{ value: production.chosen, label: `${short(production.chosen)} (not found)` }] : []),
                ...production.branches.map((ref) => ({ value: ref, label: short(ref) })),
              ],
              get: () => production.chosen ?? "",
              set: (value) => writeGit("local", [["oxbow.productionBranch", String(value) || null]]),
            },
            setting: gitSetting("local", () => [["oxbow.productionBranch", production.chosen]], ["oxbow.productionBranch"]),
          },
        ],
      },
      {
        title: "Identity",
        rows: [
          {
            id: "git:local:identity",
            label: "Use a different identity in this repository",
            sub: () => (ownIdentity() ? `Saved in ${r.name}/.git/config` : `Commits use ${who()}`),
            control: {
              type: "switch",
              get: ownIdentity,
              set: (on) =>
                writeGit(
                  "local",
                  on
                    ? [
                        ["user.name", config("global", "user.name") ?? ""],
                        ["user.email", config("global", "user.email") ?? ""],
                      ]
                    : [
                        ["user.name", null],
                        ["user.email", null],
                      ],
                ),
            },
            setting: gitSetting(
              "local",
              () => [
                ["user.name", config("local", "user.name") ?? null],
                ["user.email", config("local", "user.email") ?? null],
              ],
              ["user.name", "user.email"],
            ),
          },
          { ...gitEdit("local", "user.name", "Name", { shown: () => config("local", "user.name") ?? "", placeholder: "Your Name" }), off: () => !ownIdentity() },
          { ...gitEdit("local", "user.email", "Email", { shown: () => config("local", "user.email") ?? "", placeholder: "you@work.example" }), off: () => !ownIdentity() },
        ],
      },
      {
        title: "Protected Branches",
        foot: `Oxbow doesn’t force push them or rewrite their commits that are already pushed: no Amend, Edit Message, Drop, Squash, Split, Edit Stack, Reset or rebase of those. Revert undoes such a commit. Commits not pushed yet stay editable. Kept as ${PROTECTED_KEY} in .git/config.`,
        rows: protectedRows(r.protected ?? { branches: [], default: true }),
      },
      {
        title: "Checks Before Commit",
        foot: "Kept as oxbow.check in .git/config, in the order they run. They run after the secrets check, also for Amend, Add to Commit and Fix a Mistake; their output goes to the Console. In a Restricted repository none runs.",
        rows: checkRows(r.checks ?? [], r.restricted),
      },
      {
        title: "Remotes",
        rows: [
          ...r.remotes.map(
            (remote: RemoteInfo): Row => ({
              id: `remote:${remote.name}`,
              label: remote.name,
              mono: true,
              sub: remote.pushUrl && remote.pushUrl !== remote.fetchUrl ? `${remote.fetchUrl} · pushes to ${remote.pushUrl}` : remote.fetchUrl,
              badge: () => (remote.pushUrl && remote.pushUrl !== remote.fetchUrl ? "fetch" : "fetch · push"),
              control: { type: "button", label: "Edit", run: () => runRepo(setUrlRequest(remote)) },
              more: () => [
                { kind: "item", label: "Copy Address", icon: menuIcons.copy, run: () => copy(remote.name, remote.fetchUrl) },
                { kind: "sep" },
                { kind: "item", label: `Remove ${remote.name}…`, icon: menuIcons.drop, danger: true, run: () => runRepo(removeRemoteRequest(remote)) },
              ],
            }),
          ),
          {
            id: "remote:add",
            label: r.remotes.length ? "Add a remote" : "No remotes yet",
            sub: r.remotes.length ? undefined : "Add one to fetch, pull and push",
            control: { type: "button", label: "Add…", run: () => runRepo(addRemoteRequest(r.remotes.map((x) => x.name))) },
          },
        ],
      },
      {
        title: "Storage",
        rows: [
          {
            id: "storage:lfs",
            label: "Git LFS",
            sub: () => {
              const patterns = r.storage?.lfsPatterns ?? [];
              return patterns.length ? `${patterns.length} ${patterns.length === 1 ? "pattern" : "patterns"}: ${patterns.join(", ")}` : "Not used: .gitattributes hands no files to Git LFS";
            },
          },
          ...(r.storage
            ? [
                {
                  id: "storage:size",
                  label: `${size(r.storage.bytes)} · ${r.storage.objects.toLocaleString("en-US")} objects`,
                  sub: r.storage.loose ? `${r.storage.loose.toLocaleString("en-US")} loose objects can be packed` : "Everything is packed",
                  control: { type: "button" as const, label: "Optimize", run: () => runRepo(optimizeRequest(r.storage!.loose)) },
                },
              ]
            : []),
        ],
      },
    ];
  },
};
