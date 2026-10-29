// Settings › This Repository › Repository Files: the list of the shared files and the one open
// in the editor (RepoFileEditor.svelte). Text not saved yet stays while Settings is open, so
// going back to the list and in again keeps it.

import { api } from "./api";
import { confirm, type Part, type Request } from "./confirm.svelte";
import { Latest } from "./latest";
import { rowSub } from "./repoFiles";
import type { Row, Section } from "./settingsRows";
import type { RepoFileKind, RepoFiles } from "./types";

export const repoFiles = $state({
  /** The repository the list is of, by its path. */
  repo: null as string | null,
  list: null as RepoFiles | null,
  problem: null as string | null,
  /** The file open in the editor. */
  open: null as RepoFileKind | null,
  /** Text not saved yet, by file. */
  drafts: {} as Partial<Record<RepoFileKind, string>>,
});

// Loads overlap (a Save, the window's focus, the section opened): only the newest reply is shown,
// and none for a repository the window has left.
const loads = new Latest(() => repoFiles.repo);

export async function loadRepoFiles() {
  const current = loads.begin();
  try {
    const list = await api.repoFiles();
    if (!current()) return;
    repoFiles.list = list;
    repoFiles.problem = null;
  } catch (err) {
    if (current()) repoFiles.problem = String(err);
  }
}

export function openRepoFile(kind: RepoFileKind) {
  repoFiles.open = kind;
}

export function closeRepoFile() {
  repoFiles.open = null;
  loadRepoFiles();
}

/** Another repository (`repo`, by its path): what was typed for the last one goes, and a list of
 *  it still on its way isn't shown. */
export function forgetRepoFiles(repo: string | null) {
  loads.drop();
  repoFiles.repo = repo;
  repoFiles.list = null;
  repoFiles.problem = null;
  repoFiles.open = null;
  repoFiles.drafts = {};
}

/** Stop tracking committed files a .gitignore rule covers; they stay on disk. */
export function untrackRequest(paths: string[]): Request {
  const what: Part[] = paths.length === 1 ? [{ code: paths[0] }] : [`${paths.length} files`];
  return {
    title: paths.length === 1 ? `Stop tracking ${paths[0]}?` : `Stop tracking ${paths.length} files?`,
    body: [
      "Git forgets ",
      ...what,
      ". It stays on your disk and leaves the repository with the next commit; when others pull that commit, git deletes it from their copies.",
    ],
    icon: "ignore",
    button: "Stop Tracking",
    // Taking files out of git reaches the team: always shown first.
    always: true,
    status: "Stopping tracking…",
    done: paths.length === 1 ? `Git no longer tracks ${paths[0]}.` : `Git no longer tracks ${paths.length} files.`,
    action: { kind: "untrack", paths },
  };
}

export async function untrack(paths: string[]) {
  await confirm.run(untrackRequest(paths));
}

const FILE_ICON = "M4 1.8h5l3 3v9.4H4zM9 1.8v3h3M6 8h4M6 10.5h4";

function fileRow(kind: RepoFileKind): Row {
  const file = () => repoFiles.list?.files.find((f) => f.kind === kind);
  const gitlab = () => repoFiles.list?.gitlab ?? false;
  return {
    id: `repoFile:${kind}`,
    label: () => file()?.path ?? "",
    mono: true,
    sub: () => {
      const f = file();
      return f ? rowSub(f, gitlab()) : "";
    },
    subTone: () => (file()?.problem ? "error" : undefined),
    file: () => ({ missing: !file()?.exists }),
    badge: () => (repoFiles.drafts[kind] !== undefined ? "Not saved" : file()?.uncommitted ? "Not committed" : file()?.exists ? null : "Not created"),
    control: {
      type: "button",
      label: () => (kind === "gitmodules" ? "View" : file()?.exists || repoFiles.drafts[kind] !== undefined ? "Edit" : "Create…"),
      run: () => openRepoFile(kind),
    },
    off: () => kind === "gitmodules" && !file()?.exists,
  };
}

export const repoFilesSection: Section = {
  id: "repoFiles",
  label: "Repository Files",
  color: "var(--section-blue)",
  icon: FILE_ICON,
  child: true,
  groups: () => [
    {
      title: "Git",
      rows: (["gitignore", "gitattributes", "mailmap", "blameIgnoreRevs", "lfsconfig", "gitmodules"] as RepoFileKind[]).map(fileRow),
    },
    {
      title: repoFiles.list?.gitlab ? "GitLab and editors" : "GitHub and editors",
      foot: "These files are committed, so everyone who clones the repository gets them. Save only writes the file: it shows up in Changes, and you commit it yourself.",
      rows: (["codeowners", "editorconfig"] as RepoFileKind[]).map(fileRow),
    },
  ],
};
