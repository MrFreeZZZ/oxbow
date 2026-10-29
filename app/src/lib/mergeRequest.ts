// Create Merge Request on GitLab: the push itself asks for it with GitLab's push options
// (`git push -o merge_request.create …`), so it needs no sign-in and works on a GitLab server of
// your own too. With nothing to push, git sends no options: GitLab's New merge request page opens
// in the browser instead, with both branches picked.

import { api } from "./api";
import { github } from "./github.svelte";
import type { Part, Request } from "./confirm.svelte";
import { hostName, pushRecovery, type RemoteContext } from "./remote";
import type { BranchContext } from "./branches";
import { mergeRequestLink, mergeRequestTitle, newMergeRequestUrl } from "./gitlab";
import type { Action, GitLabProject, History, MergeRequest, RefInfo } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export interface NewMergeRequest extends MergeRequest {
  /** Open the merge request in the browser once GitLab has it. */
  open: boolean;
}

export interface MergeRequestContext extends RemoteContext {
  branch: string;
  project: GitLabProject;
  /** Branches it can go into, the trunk first. */
  targets: string[];
  colorOf: (name: string) => number;
  /** Only the remote has the branch: there is nothing to push, GitLab's page opens. */
  remoteOnly?: boolean;
}

/** The GitLab project `ref` pushes to: its upstream's remote, else the default remote. */
export function gitlabOf(history: History, projects: GitLabProject[], ref: RefInfo): GitLabProject | null {
  const t = ref.tracking && !ref.tracking.gone ? ref.tracking : null;
  const remote = t?.remote ?? history.defaultRemote;
  return projects.find((p) => p.remote === remote) ?? null;
}

/** Whether Oxbow is signed in to `project`'s server: then Create Merge Request opens the full
 *  form on the Merge Request screen rather than asking GitLab through the push. */
export function signedInTo(project: GitLabProject): boolean {
  const signed = github.mrProject;
  return !!signed && signed.remote === project.remote && signed.host === project.host;
}

/** Create Merge Request for a local branch, from the branch list or the graph: into the trunk,
 *  or a branch like develop that the remote has. */
export function branchMergeRequest(ctx: BranchContext, ref: RefInfo, project: GitLabProject): Request {
  const history = ctx.history;
  // A branch only the remote has, e.g. origin/sigma-tg: it is already there.
  const remoteOnly = ref.kind === "remote";
  const name = remoteOnly ? ref.name.slice(project.remote.length + 1) : ref.name;
  const onRemote = (branch: string) => history.refs.some((r) => r.kind === "remote" && r.name === `${project.remote}/${branch}`);
  const trunk = history.trunk ?? "main";
  const targets = [trunk, ...["develop", "dev", "staging"].filter(onRemote)].filter((branch, i, all) => branch !== name && all.indexOf(branch) === i);
  const summary = history.rows.find((r) => r.id === ref.target)?.summary;
  return mergeRequestRequest(
    {
      branch: name,
      color: ctx.colorOf(ref.name),
      tracking: remoteOnly ? { remote: project.remote, branch: name, ahead: 0, behind: 0, gone: false, rewritten: null, replay: null } : ref.tracking,
      remote: project.remote,
      remotes: history.remotes,
      unpushed: remoteOnly ? [] : history.rows.filter((r) => r.unpushed && !r.worktree && r.graph.branch === ref.name),
      project,
      targets,
      colorOf: ctx.colorOf,
      remoteOnly,
    },
    { target: targets[0] ?? trunk, title: mergeRequestTitle(summary, name), draft: true, removeSource: false, open: true },
  );
}

export function mergeRequestRequest(ctx: MergeRequestContext, state: NewMergeRequest): Request {
  const again = (change: Partial<NewMergeRequest>) => mergeRequestRequest(ctx, { ...state, ...change });
  const { branch, project } = ctx;
  const host = hostName(project.host);
  const chip = (name: string): Part => ({ branch: name, color: ctx.colorOf(name) });
  const t = ctx.tracking && !ctx.tracking.gone ? ctx.tracking : null;
  const upstream = t?.branch ?? branch;
  const remote = project.remote;
  const fields: Request["fields"] = [
    { label: "Title", text: { value: state.title, placeholder: branch, edit: (value) => again({ title: value }) } },
    { label: "Into", chips: ctx.targets.map((name) => ({ label: name, on: name === state.target, pick: () => again({ target: name }) })) },
  ];
  const toPush = t ? t.ahead : Math.max(ctx.unpushed.length, 1);

  // Nothing new: git says "Everything up-to-date" and never sends the options.
  if (t && !toPush) {
    const url = newMergeRequestUrl(project, upstream, state.target, state.title);
    return {
      title: `Open a merge request for ${branch}?`,
      body: ctx.remoteOnly
        ? [`Opens ${host}’s New merge request page with `, chip(`${remote}/${upstream}`), " going into ", chip(state.target), "."]
        : [
            chip(`${remote}/${upstream}`),
            ` already has every commit of `,
            chip(branch),
            `, so there is nothing to push that could ask ${host} for it. Opens ${host}’s New merge request page with it going into `,
            chip(state.target),
            ".",
          ],
      icon: "push",
      button: "Open in Browser",
      fields,
      status: `Opening ${host}…`,
      local: {
        commands: [`open '${url}'`],
        comment: `${host}’s New merge request page, the branches picked`,
        run: async () => {
          await api.openGitLab(url);
          return `Opened ${host}’s New merge request page.`;
        },
      },
    };
  }

  const request: MergeRequest = { target: state.target, title: state.title.trim(), draft: state.draft, removeSource: state.removeSource };
  const action: Action & { kind: "push" } = {
    kind: "push",
    remote,
    branch,
    upstream,
    setUpstream: !t,
    force: false,
    lease: null,
    noVerify: false,
    mergeRequest: request,
  };
  const body: Part[] = [
    t ? `Pushes ${plural(toPush, "commit")} of ` : "Publishes ",
    chip(branch),
    t ? ` to ${remote}` : ` to ${remote} as `,
    ...(t ? [] : [chip(`${remote}/${upstream}`)]),
    `, and ${host} opens a ${state.draft ? "draft " : ""}merge request of it into `,
    chip(state.target),
    ".",
  ];
  if (state.removeSource) body.push(` Once it is merged, ${host} deletes `, chip(`${remote}/${upstream}`), ".");
  return {
    title: `Create a merge request for ${branch}?`,
    body,
    icon: "push",
    button: t ? "Push and Create" : "Publish and Create",
    fields,
    options: [
      { label: "Draft", sub: "Reviewers see it isn’t ready to merge yet", on: state.draft, toggle: () => again({ draft: !state.draft }) },
      { label: `Delete the branch on ${host} once merged`, on: state.removeSource, toggle: () => again({ removeSource: !state.removeSource }) },
      { label: "Open it in the browser", on: state.open, toggle: () => again({ open: !state.open }) },
    ],
    note: `${host} reads the -o options of the push: no sign-in to ${host} is needed.`,
    invalid: request.title ? null : "Give it a title",
    status: `Pushing ${branch} to ${remote}…`,
    said: (output) => {
      const link = mergeRequestLink(output, project);
      if (link) return `${host} has merge request !${link.number} of ${branch}.`;
      return `Pushed ${branch}, but ${host} didn’t answer with a merge request, so its New merge request page opens.`;
    },
    after: (output) => {
      const link = mergeRequestLink(output, project);
      const url = link ? (state.open ? link.url : null) : newMergeRequestUrl(project, upstream, state.target, state.title);
      if (url) api.openGitLab(url).catch(() => {});
      // Signed in there: the branch shows its !N.
      if (link) github.refresh();
    },
    recover: (failure) => pushRecovery(failure, ctx, action),
    action,
  };
}
