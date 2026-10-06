// Confirmations for Settings › This Repository: remotes and storage. They change the repository,
// so they go through the same sheet as every other action.

import type { Request } from "./confirm.svelte";
import type { RemoteInfo } from "./types";

const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

/** Add a remote: its name and address. */
export function addRemoteRequest(taken: string[], name = "", url = ""): Request {
  const again = (n: string, u: string) => addRemoteRequest(taken, n, u);
  const n = name.trim();
  const u = url.trim();
  const nameProblem = !n ? null : !NAME.test(n) ? "Letters, digits, dots, dashes and underscores only." : taken.includes(n) ? `There already is a remote called ${n}.` : null;
  return {
    title: "Add a Remote",
    body: n && !nameProblem ? ["Adds ", { code: n }, u ? [" at ", { code: u }, "."] : "."].flat() : ["Adds a remote this repository can fetch from and push to."],
    icon: "remote",
    button: "Add Remote",
    fields: [
      { label: "Name", text: { value: name, placeholder: taken.length ? "upstream" : "origin", edit: (value) => again(value, url) }, error: nameProblem ?? undefined },
      { label: "Address", text: { value: url, placeholder: "git@github.com:owner/repo.git", edit: (value) => again(name, value) } },
    ],
    invalid: !n ? "Name the remote" : nameProblem ? nameProblem : !u ? "Give its address" : null,
    status: `Adding ${n}…`,
    done: `Added ${n}. Fetch to see its branches.`,
    action: { kind: "addRemote", name: n, url: u },
  };
}

/** Point a remote at another address. */
export function setUrlRequest(remote: RemoteInfo, url = remote.fetchUrl): Request {
  const u = url.trim();
  return {
    title: `Change the Address of ${remote.name}`,
    body: ["Fetch and push to ", { code: remote.name }, " will use the new address. Its branches stay as they are."],
    icon: "edit",
    button: "Change Address",
    fields: [{ label: "Address", text: { value: url, placeholder: remote.fetchUrl, edit: (value) => setUrlRequest(remote, value) } }],
    invalid: !u ? "Give its address" : u === remote.fetchUrl && u === remote.pushUrl ? "The address is the same" : null,
    status: `Changing ${remote.name}…`,
    done: `${remote.name} now points at ${u}`,
    action: { kind: "setRemoteUrl", name: remote.name, url: u },
  };
}

export function removeRemoteRequest(remote: RemoteInfo): Request {
  return {
    title: `Remove the remote ${remote.name}?`,
    body: [
      "Forgets ",
      { code: remote.name },
      " and its remote branches here. Nothing on the server changes, and your local branches stay; ones that tracked it no longer have an upstream.",
    ],
    icon: "drop",
    button: "Remove Remote",
    danger: true,
    status: `Removing ${remote.name}…`,
    done: `Removed ${remote.name}`,
    action: { kind: "removeRemote", name: remote.name },
  };
}

export function optimizeRequest(loose: number): Request {
  return {
    title: "Optimize the repository?",
    body: [
      loose
        ? `Packs ${loose.toLocaleString("en-US")} loose ${loose === 1 ? "object" : "objects"} and removes ones no branch, tag or stash needs any more. Commits stay as they are.`
        : "Repacks the objects and removes ones no branch, tag or stash needs any more. Commits stay as they are.",
    ],
    icon: "box",
    button: "Optimize",
    status: "Optimizing…",
    done: "Optimized",
    action: { kind: "optimize" },
  };
}
