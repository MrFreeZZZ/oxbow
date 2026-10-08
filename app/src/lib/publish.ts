// Publish to GitHub: make a repository there for one that has no remote yet, add it as origin
// and push the checked-out branch. The sheet shows the API request as curl and the git commands
// that follow; `gh repo create … --source . --push` does the same.

import { api } from "./api";
import { confirm, type Part, type Request } from "./confirm.svelte";
import { prefs } from "./prefs.svelte";
import type { GitHubOwners, Publish } from "./types";

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
      if (!made)
        return {
          title: "GitHub didn’t make the repository",
          body: [failure.output.trim()],
          icon: "warn",
          tone: "err",
          alt: { label: "Change and Try Again…", request: () => publishRequest(path, owners, publish, color) },
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

/** The request to start with, or a message when nobody is signed in. */
export async function startPublish(path: string, name: string, branch: string, color: number): Promise<Request> {
  const owners = await api.githubOwners();
  return publishRequest(path, owners, { owner: owners.login, personal: true, name: repositoryName(name), private: true, description: "", branch }, color);
}
