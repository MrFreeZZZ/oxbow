// Publish to GitLab as plain logic: the request and commands its sheet shows, exactly as the
// backend runs them, the `glab` command that makes the same project, and who can see it.

import type { GitLabPublish, GitLabVisibility } from "./types";

/** Whether GitLab takes the name as a project's path: letters, digits, - _ and ., not starting
 *  with - and not ending in .git or .atom. */
export function validProjectName(name: string): boolean {
  return /^[A-Za-z0-9._-]{1,100}$/.test(name) && name !== "." && name !== ".." && !name.startsWith("-") && !/\.(git|atom)$/i.test(name);
}

const quote = (text: string) => `'${text.replace(/'/g, "'\\''")}'`;

/** The request and commands, exactly as the backend runs them; `web` is the server's address. */
export function labPublishCommands(publish: GitLabPublish, web: string, hooks: boolean): string[] {
  const body: Record<string, unknown> = {};
  // serde_json writes keys in alphabetical order.
  if (publish.description.trim()) body.description = publish.description.trim();
  body.name = publish.name;
  body.namespace_id = publish.namespaceId;
  body.path = publish.name;
  body.visibility = publish.visibility;
  return [
    `curl -X POST -H "PRIVATE-TOKEN: $GITLAB_TOKEN" ${web}/api/v4/projects -d ${quote(JSON.stringify(body))}`,
    `git remote add origin ${web}/${publish.namespace}/${publish.name}.git`,
    `git push${hooks ? "" : " --no-verify"} -u origin ${publish.branch}`,
  ];
}

export function labPublishNote(publish: GitLabPublish): string {
  return `glab repo create ${publish.namespace}/${publish.name} --${publish.visibility} makes the same project`;
}

/** Who can see the project, for the sheet. */
export function visibilityText(visibility: GitLabVisibility, namespace: string, personal: boolean, host: string): string {
  if (visibility === "private") return personal ? "Private: only you, and people you add, can see it." : `Private: members of ${namespace} can see it.`;
  if (visibility === "internal") return `Internal: anyone signed in to ${host} can see it.`;
  return "Public: anyone can see it and its whole history.";
}
