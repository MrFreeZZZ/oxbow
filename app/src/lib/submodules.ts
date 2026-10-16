// Submodules: other repositories checked out in a folder here, at the commit this one records.
// What the sidebar says about each, and the requests that update, sync, add and remove them.

import { api } from "./api";
import { confirm, type Part, type Request } from "./confirm.svelte";
import { shortId } from "./format";
import type { Submodule, SubmoduleMove, UnpushedSubmodule } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** What the note at the end of a submodule's row says; the most pressing state wins. */
export function submoduleNote(sub: Submodule): { text: string; warn?: boolean; muted?: boolean } | null {
  if (!sub.cloned) return { text: "not cloned", muted: true };
  if (sub.missing) return { text: "needs fetch", warn: true };
  if (sub.current !== sub.recorded) return { text: "out of sync", warn: true };
  if (sub.changed) return { text: `${sub.changed} changed` };
  return null;
}

/** The second line of a submodule's row: where its folder is. */
export function submoduleWhere(sub: Submodule): string {
  if (!sub.cloned) return "folder is empty";
  const at = sub.current ? shortId(sub.current) : "no commit";
  if (sub.recorded && sub.current !== sub.recorded) return `has ${at} · needs ${shortId(sub.recorded)}`;
  return `${at} · ${sub.branch ?? "detached"}`;
}

/** The branch Pull Latest takes, in words. */
export function followsWord(sub: { follows: string | null }): string {
  if (sub.follows === ".") return "the branch checked out here";
  return sub.follows ?? "its default branch";
}

/** The repository's own name, from its folder. */
const nameOf = (repoPath: string) => repoPath.split(/[\\/]/).filter(Boolean).pop() ?? "this repository";

/** Check out the recorded commit in `sub`'s folder, cloning it first when it isn't. All of them without `sub`. */
export function updateRequest(subs: Submodule[], sub: Submodule | null, repoPath: string): Request {
  const repo = nameOf(repoPath);
  if (!sub) {
    const fresh = subs.filter((s) => !s.cloned).length;
    return {
      title: "Update all submodules?",
      body: [
        `Checks out the commit ${repo} records in each of its ${plural(subs.length, "submodule")}`,
        ...(fresh ? [`, cloning the ${plural(fresh, "one")} not cloned yet first`] : []),
        ". Their own submodules too.",
      ],
      icon: "box",
      button: "Update All",
      status: "Updating submodules…",
      done: "The submodules are at the commits this repository records.",
      past: "Updated submodules",
      action: { kind: "submoduleUpdate", paths: [], latest: false },
    };
  }
  const to = sub.recorded ? shortId(sub.recorded) : "the recorded commit";
  const body: Part[] = sub.cloned
    ? ["Checks out ", { code: to }, ` in ${sub.title}, the commit ${repo} records. Commits made there stay in its history.`]
    : ["Clones ", { code: sub.url ?? sub.path }, " into ", { code: sub.path }, " and checks out ", { code: to }, `, the commit ${repo} records.`];
  if (sub.changed) body.push(` Its ${plural(sub.changed, "changed file")} stop the update if the checkout would overwrite them.`);
  return {
    title: sub.cloned ? `Update ${sub.title} to ${to}?` : `Clone ${sub.title}?`,
    body,
    icon: "box",
    button: sub.cloned ? "Update" : "Clone",
    status: sub.cloned ? `Updating ${sub.title}…` : `Cloning ${sub.title}…`,
    done: `${sub.title} is at ${to}.`,
    past: sub.cloned ? `Updated ${sub.title} to ${to}` : `Cloned ${sub.title}`,
    action: { kind: "submoduleUpdate", paths: [sub.path], latest: false },
  };
}

/** From Changes: put a submodule's folder back at the commit the index records. */
export function updateFolderRequest(move: SubmoduleMove): Request {
  const to = move.old ? shortId(move.old) : "the recorded commit";
  return {
    title: `Update ${move.title} to ${to}?`,
    body: ["Checks out ", { code: to }, ` in ${move.title}, the commit recorded here, so it no longer shows as changed. Commits made there stay in its history.`],
    icon: "box",
    button: "Update",
    status: `Updating ${move.title}…`,
    done: `${move.title} is at ${to}.`,
    past: `Updated ${move.title} to ${to}`,
    action: { kind: "submoduleUpdate", paths: [move.path], latest: false },
  };
}

/** Fetch `sub` and check out the newest commit of the branch it follows; committing records it. */
export function latestRequest(sub: Submodule, repoPath: string): Request {
  const branch = followsWord(sub);
  return {
    title: `Pull the latest ${sub.title}?`,
    body: [
      "Fetches ",
      { code: sub.url ?? sub.path },
      ` and checks out the newest commit of ${branch} in ${sub.title}. Then commit it in ${nameOf(repoPath)} to record the new commit.`,
    ],
    icon: "pull",
    button: "Pull Latest",
    status: `Pulling the latest ${sub.title}…`,
    done: `${sub.title} is at the newest commit of ${branch}. Stage it to record it.`,
    past: `Pulled the latest ${sub.title}`,
    action: { kind: "submoduleUpdate", paths: [sub.path], latest: true },
  };
}

/** Copy the URL from `.gitmodules` to the submodule's settings, after it moved; all without `sub`. */
export function syncRequest(sub: Submodule | null): Request {
  return {
    title: sub ? `Sync the URL of ${sub.title}?` : "Sync all submodule URLs?",
    body: [
      "Takes the address from ",
      { code: ".gitmodules" },
      sub ? ` for ${sub.title}` : " for every submodule",
      ", so fetches go where this repository says now. Use it after a submodule moved to another address.",
    ],
    icon: "remote",
    button: "Sync",
    status: "Syncing submodule URLs…",
    done: sub ? `${sub.title} fetches from ${sub.url ?? "its new address"}.` : "The submodules fetch from the addresses in .gitmodules.",
    past: sub ? `Synced the URL of ${sub.title}` : "Synced submodule URLs",
    action: { kind: "submoduleSync", paths: sub ? [sub.path] : [] },
  };
}

/** Remove a submodule: its folder, its settings and its `.gitmodules` entry, staged. */
export function removeSubmoduleRequest(sub: Submodule): Request {
  const body: Part[] = ["Deletes the folder ", { code: sub.path }, " and its entry in ", { code: ".gitmodules" }, ". Commit the result to remove it for everyone."];
  if (sub.changed) body.push(` ${plural(sub.changed, "file")} changed inside ${sub.title} ${sub.changed === 1 ? "is" : "are"} lost with the folder.`);
  return {
    title: `Remove the submodule ${sub.title}?`,
    body,
    icon: "drop",
    button: "Remove",
    danger: true,
    note: `Its commits stay in .git/modules, so adding ${sub.title} back is quick.`,
    status: `Removing ${sub.title}…`,
    done: `Removed ${sub.title}. Commit to record it.`,
    action: { kind: "submoduleRemove", path: sub.path },
  };
}

interface NewSubmodule {
  url: string;
  folder: string;
  /** The folder was typed, so it stops following the URL. */
  edited: boolean;
  branch: string;
}

/** The last part of a URL without `.git`: git@github.com:acme/telemetry.git → telemetry. */
function repoName(url: string): string {
  return (
    url
      .trim()
      .replace(/[/\\]+$/, "")
      .split(/[/\\:]/)
      .pop()
      ?.replace(/\.git$/, "") ?? ""
  );
}

/** What the last check of a URL found, so the sheet can say it while it is still the URL typed. */
let probed: { url: string; text: string; error: boolean } | null = null;
let probing = "";
/** Number of the sheet opened last, and what is typed in it now: a check that answers late
 *  updates that, never the fields as they were when it started, nor a sheet closed since. */
let form = 0;
let typed: NewSubmodule | null = null;

/** The Add Submodule sheet; without `state`, a new one. */
export function addSubmoduleRequest(subs: Submodule[], repoPath: string, state?: NewSubmodule): Request {
  if (!state) {
    form++;
    probing = "";
    state = { url: "", folder: "", edited: false, branch: "" };
  }
  typed = state;
  const again = (change: Partial<NewSubmodule>) => addSubmoduleRequest(subs, repoPath, { ...state, ...change });
  const url = state.url.trim();
  const folder = state.folder.trim().replace(/^\.\//, "").replace(/[/\\]+$/, "");
  const branch = state.branch.trim();
  let invalid: string | null = null;
  let folderError: string | undefined;
  if (!url) invalid = "Type the address of the repository.";
  else if (!folder) invalid = "Choose a folder for it.";
  else if (/^([/\\]|[A-Za-z]:)/.test(folder) || folder.split(/[/\\]/).includes("..")) invalid = folderError = "A folder inside this repository, like libs/name.";
  else if (subs.some((s) => s.path === folder)) invalid = folderError = "That folder is a submodule already.";

  const known = probed && probed.url === url ? probed : null;
  if (url && !known && probing !== url) {
    probing = url;
    const mine = form;
    // Checked after typing stops for a moment, then shown if the URL is still the one typed.
    const show = () => {
      if (mine === form && probing === url && typed?.url.trim() === url && confirm.request?.action?.kind === "submoduleAdd") {
        confirm.change(addSubmoduleRequest(subs, repoPath, typed));
      }
    };
    setTimeout(() => {
      if (mine !== form || probing !== url) return;
      api.probeRemote(url).then(
        (probe) => {
          probed = { url, error: false, text: `Found${probe.defaultBranch ? ` · default branch ${probe.defaultBranch}` : " · empty repository"}` };
          show();
        },
        () => {
          probed = { url, error: true, text: "Can’t reach it: check the address, or your access to it." };
          show();
        },
      );
    }, 500);
  }

  const name = folder.split(/[/\\]/).pop() || repoName(url) || "it";
  return {
    title: "Add submodule",
    body: [`Clones another repository into a folder here; ${nameOf(repoPath)} records which of its commits it uses.`],
    icon: "box",
    button: "Add",
    fields: [
      {
        label: "URL",
        text: {
          value: state.url,
          placeholder: "git@github.com:acme/telemetry.git",
          focus: !state.url,
          edit: (value) => again({ url: value, ...(state.edited ? {} : { folder: repoName(value) }) }),
        },
        note: known && !known.error ? known.text : undefined,
        error: known?.error ? known.text : undefined,
      },
      { label: "Folder", text: { value: state.folder, placeholder: "libs/telemetry", edit: (value) => again({ folder: value, edited: true }) }, error: folderError },
      {
        label: "Branch",
        text: { value: state.branch, placeholder: "Its default branch", edit: (value) => again({ branch: value }) },
        note: "Pull Latest takes new commits from this branch.",
      },
    ],
    invalid,
    note: "The commit that adds it is yours to make: .gitmodules and the folder are staged.",
    status: `Cloning ${name}…`,
    done: `Added ${name}. Commit to record it.`,
    action: { kind: "submoduleAdd", url, path: folder, branch: branch || null },
  };
}

/** The warning on the push sheet about submodule commits their remotes don't have. */
export function unpushedWarning(subs: UnpushedSubmodule[], button = "Push Both"): Part[] {
  const one = subs.length === 1;
  const names: Part[] = [];
  subs.forEach((s, i) => {
    if (i) names.push(i === subs.length - 1 ? " and " : ", ");
    names.push(s.title, " ", { code: shortId(s.commit) });
  });
  return [
    ...names,
    one ? " isn’t on its remote yet. " : " aren’t on their remotes yet. ",
    "Teammates who pull couldn’t check ",
    one ? "it" : "them",
    ` out. ${button} sends `,
    one ? "it" : "them",
    " first.",
  ];
}
