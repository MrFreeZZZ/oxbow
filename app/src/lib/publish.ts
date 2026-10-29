// Publish to GitHub: make a repository there for one that has no remote yet, add it as origin
// and push the checked-out branch. The sheet shows the API request as curl and the git commands
// that follow; `gh repo create … --source . --push` does the same. Publish to GitLab the same
// way, as a project in one of the account's namespaces.

import { api } from "./api";
import { confirm, type Part, type Request } from "./confirm.svelte";
import { prefs } from "./prefs.svelte";
import { labPublishCommands, labPublishNote, validProjectName, visibilityText } from "./labPublish";
import type { GitHubOwners, GitLabAccount, GitLabNamespace, GitLabPublish, GitLabVisibility, Publish } from "./types";

/** Whether GitHub takes the name as it is. */
export function validName(name: string): boolean {
  return /^[A-Za-z0-9._-]{1,100}$/.test(name) && name !== "." && name !== "..";
}

/** GitHub's own rule for a typed name: other characters become dashes. */
export function repositoryName(name: string): string {
  return name.trim().replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "repository";
}

const quote = (text: string) => `'${text.replace(/'/g, "'\\''")}'`;

/** The request and commands, exactly as the backend runs them. */
export function publishCommands(publish: Publish): string[] {
  const body: Record<string, unknown> = {};
  // serde_json writes keys in order.
  if (publish.description.trim()) body.description = publish.description.trim();
  body.name = publish.name;
  body.private = publish.private;
  const path = publish.personal ? "/user/repos" : `/orgs/${publish.owner}/repos`;
  return [
    `curl -X POST -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com${path} -d ${quote(JSON.stringify(body))}`,
    `git remote add origin https://github.com/${publish.owner}/${publish.name}.git`,
    `git push${prefs.get("oxbow.git.runHooks") ? "" : " --no-verify"} -u origin ${publish.branch}`,
  ];
}

export function publishNote(publish: Publish): string {
  return `gh repo create ${publish.owner}/${publish.name} --${publish.private ? "private" : "public"} --source . --push does the same`;
}

/** The confirmation for a repository with no remote. `color` is the branch's lane. */
export function publishRequest(path: string, owners: GitHubOwners, publish: Publish, color: number): Request {
  const at = `${publish.owner}/${publish.name}`;
  const again = (change: Partial<Publish>) => publishRequest(path, owners, { ...publish, ...change }, color);
  const who: Part[] = publish.private
    ? [publish.personal ? " Private: only you, and people you invite, can see it." : ` Private: members of ${publish.owner} with access can see it.`]
    : [" Public: anyone can see it and its whole history."];
  return {
    title: "Publish to GitHub?",
    body: ["Creates ", { code: at }, " on GitHub, adds it as ", { code: "origin" }, " and pushes ", { branch: publish.branch, color }, " there.", ...who],
    icon: "push",
    button: "Publish",
    fields: [
      {
        label: "Name",
        text: { value: publish.name, placeholder: "my-project", edit: (name) => again({ name }) },
        error: publish.name && !validName(publish.name) ? `GitHub takes letters, digits, - _ and . only: try ${repositoryName(publish.name)}` : undefined,
      },
      ...(owners.orgs.length
        ? [
            {
              label: "Owner",
              chips: [owners.login, ...owners.orgs].map((owner) => ({
                label: owner,
                on: owner === publish.owner,
                pick: () => again({ owner, personal: owner === owners.login }),
              })),
            },
          ]
        : []),
      {
        label: "Visibility",
        chips: [
          { label: "Private", on: publish.private, pick: () => again({ private: true }) },
          { label: "Public", on: !publish.private, pick: () => again({ private: false }) },
        ],
      },
      { label: "Description", text: { value: publish.description, placeholder: "Optional", edit: (description) => again({ description }) } },
    ],
    invalid: !publish.name ? "Name the repository" : !validName(publish.name) ? "Use letters, digits, - _ and . for the name" : null,
    note: "Undo can’t take the GitHub repository back: delete it on GitHub if you change your mind.",
    status: `Publishing to github.com/${at}…`,
    recover: (failure) => {
      // GitHub made nothing: the name can change and it can go again.
      const made = confirm.lines.some((line) => line.text.startsWith("Created https://"));
      // The push only starts once origin is added.
      const linked = confirm.lines.some((line) => line.kind === "cmd" && line.text.startsWith("git push"));
      if (!made)
        return {
          title: "GitHub didn’t make the repository",
          body: [failure.output.trim()],
          icon: "warn",
          tone: "err",
          alt: { label: "Change and Try Again…", request: () => publishRequest(path, owners, publish, color) },
          close: "Close",
        };
      if (!linked)
        return {
          title: `github.com/${at} is made, but origin wasn’t added`,
          body: [failure.output.trim()],
          icon: "warn",
          tone: "err",
          alt: { label: "Add Origin and Push…", request: () => linkRequest(path, publish, color) },
          close: "Close",
        };
      return {
        title: `github.com/${at} is made, but the push failed`,
        body: ["The repository is on GitHub and ", { code: "origin" }, " points at it. Push again from the toolbar once the problem is fixed."],
        icon: "warn",
        tone: "err",
        close: "Close",
      };
    },
    local: {
      commands: publishCommands(publish),
      comments: [
        `GitHub makes an empty repository; ${publishNote(publish)}`,
        "the new GitHub repository becomes origin",
        `-u: remember origin/${publish.branch} as the upstream`,
      ],
      run: async () => {
        await api.githubPublish(path, publish);
        return `Published to github.com/${at}.`;
      },
    },
  };
}

/** After a Publish that made the repository on GitHub but stopped before origin was added: add
 *  it and push, without making the repository again. */
function linkRequest(path: string, publish: Publish, color: number): Request {
  const at = `${publish.owner}/${publish.name}`;
  const [, ...commands] = publishCommands(publish);
  return {
    title: `Add github.com/${at} as origin?`,
    body: ["The repository is on GitHub already. This adds it as ", { code: "origin" }, " and pushes ", { branch: publish.branch, color }, " there."],
    icon: "push",
    button: "Add and Push",
    status: `Pushing to github.com/${at}…`,
    local: {
      commands,
      comments: ["the GitHub repository becomes origin", `-u: remember origin/${publish.branch} as the upstream`],
      run: async () => {
        await api.githubPublish(path, publish, false);
        return `Published to github.com/${at}.`;
      },
    },
  };
}

/** The request to start with, or a message when nobody is signed in. */
export async function startPublish(path: string, name: string, branch: string, color: number): Promise<Request> {
  const owners = await api.githubOwners();
  return publishRequest(path, owners, { owner: owners.login, personal: true, name: repositoryName(name), private: true, description: "", branch }, color);
}

/** A GitLab server signed in to, with the namespaces its account can make a project in. */
export interface LabPlace {
  account: GitLabAccount;
  spaces: GitLabNamespace[];
}

/** The confirmation of Publish to GitLab, on one of the servers in `places`. */
export function publishGitLabRequest(path: string, places: LabPlace[], at: LabPlace, publish: GitLabPublish, color: number): Request {
  const again = (change: Partial<GitLabPublish>, place = at) => publishGitLabRequest(path, places, place, { ...publish, ...change }, color);
  const { host, web } = at.account;
  const where = `${host}/${publish.namespace}/${publish.name}`;
  const personal = at.spaces.find((n) => n.id === publish.namespaceId)?.kind === "user";
  const VISIBILITY: [GitLabVisibility, string][] = [
    ["private", "Private"],
    ["internal", "Internal"],
    ["public", "Public"],
  ];
  return {
    title: "Publish to GitLab?",
    body: ["Creates ", { code: `${publish.namespace}/${publish.name}` }, ` on ${host}, adds it as `, { code: "origin" }, " and pushes ", { branch: publish.branch, color }, " there. ", visibilityText(publish.visibility, publish.namespace, personal, host)],
    icon: "push",
    button: "Publish",
    fields: [
      {
        label: "Name",
        text: { value: publish.name, placeholder: "my-project", edit: (name) => again({ name }) },
        error: publish.name && !validProjectName(publish.name) ? `GitLab takes letters, digits, - _ and . only, not first -: try ${repositoryName(publish.name)}` : undefined,
      },
      ...(places.length > 1
        ? [
            {
              label: "Server",
              chips: places.map((p) => ({
                label: p.account.host,
                on: p === at,
                pick: () => again({ namespaceId: p.spaces[0]?.id ?? 0, namespace: p.spaces[0]?.fullPath ?? p.account.username }, p),
              })),
            },
          ]
        : []),
      ...(at.spaces.length > 1
        ? [
            {
              label: "Namespace",
              chips: at.spaces.map((n) => ({ label: n.fullPath, on: n.id === publish.namespaceId, pick: () => again({ namespaceId: n.id, namespace: n.fullPath }) })),
            },
          ]
        : []),
      {
        label: "Visibility",
        chips: VISIBILITY.map(([id, label]) => ({ label, on: publish.visibility === id, pick: () => again({ visibility: id }) })),
      },
      { label: "Description", text: { value: publish.description, placeholder: "Optional", edit: (description) => again({ description }) } },
    ],
    invalid: !publish.name ? "Name the project" : !validProjectName(publish.name) ? "Use letters, digits, - _ and . for the name" : !publish.namespaceId ? "GitLab named no namespace to make it in" : null,
    note: "Undo can’t take the GitLab project back: delete it on GitLab if you change your mind.",
    status: `Publishing to ${where}…`,
    recover: (failure) => {
      const made = confirm.lines.some((line) => line.text.startsWith("Created http"));
      const linked = confirm.lines.some((line) => line.kind === "cmd" && line.text.startsWith("git push"));
      if (!made)
        return {
          title: "GitLab didn’t make the project",
          body: [failure.output.trim()],
          icon: "warn",
          tone: "err",
          alt: { label: "Change and Try Again…", request: () => publishGitLabRequest(path, places, at, publish, color) },
          close: "Close",
        };
      if (!linked)
        return {
          title: `${where} is made, but origin wasn’t added`,
          body: [failure.output.trim()],
          icon: "warn",
          tone: "err",
          alt: { label: "Add Origin and Push…", request: () => linkGitLabRequest(path, at, publish, color) },
          close: "Close",
        };
      return {
        title: `${where} is made, but the push failed`,
        body: ["The project is on GitLab and ", { code: "origin" }, " points at it. Push again from the toolbar once the problem is fixed."],
        icon: "warn",
        tone: "err",
        close: "Close",
      };
    },
    local: {
      commands: labPublishCommands(publish, web, prefs.get("oxbow.git.runHooks")),
      comments: [`GitLab makes an empty project; ${labPublishNote(publish)}`, "the new GitLab project becomes origin", `-u: remember origin/${publish.branch} as the upstream`],
      run: async () => {
        await api.gitlabPublish(path, host, publish);
        return `Published to ${where}.`;
      },
    },
  };
}

/** After a Publish that made the project on GitLab but stopped before origin was added. */
function linkGitLabRequest(path: string, at: LabPlace, publish: GitLabPublish, color: number): Request {
  const where = `${at.account.host}/${publish.namespace}/${publish.name}`;
  const [, ...commands] = labPublishCommands(publish, at.account.web, prefs.get("oxbow.git.runHooks"));
  return {
    title: `Add ${where} as origin?`,
    body: ["The project is on GitLab already. This adds it as ", { code: "origin" }, " and pushes ", { branch: publish.branch, color }, " there."],
    icon: "push",
    button: "Add and Push",
    status: `Pushing to ${where}…`,
    local: {
      commands,
      comments: ["the GitLab project becomes origin", `-u: remember origin/${publish.branch} as the upstream`],
      run: async () => {
        await api.gitlabPublish(path, at.account.host, publish, false);
        return `Published to ${where}.`;
      },
    },
  };
}

/** The namespaces of each GitLab server signed in to; a server that doesn't answer is left out. */
export async function labPlaces(): Promise<LabPlace[]> {
  const accounts = await api.gitlabAccounts().catch(() => [] as GitLabAccount[]);
  const all = await Promise.all(accounts.map((account) => api.gitlabNamespaces(account.host).then((spaces) => ({ account, spaces }), () => null)));
  return all.filter((p): p is LabPlace => !!p && p.spaces.length > 0);
}

/** Publish to GitLab, in the account's own namespace, private; null when no server answers. */
export async function startPublishGitLab(path: string, name: string, branch: string, color: number): Promise<Request | null> {
  const places = await labPlaces();
  const at = places[0];
  if (!at) return null;
  const own = at.spaces[0];
  return publishGitLabRequest(path, places, at, { namespaceId: own.id, namespace: own.fullPath, name: repositoryName(name), visibility: "private", description: "", branch }, color);
}
