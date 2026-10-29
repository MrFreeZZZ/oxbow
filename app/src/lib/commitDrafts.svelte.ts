// The commit message of the Changes panel, kept for each repository folder while the app runs
// and in localStorage after it quits.

export interface Draft {
  summary: string;
  description: string;
  amend: boolean;
  /** The commit Amend was turned on for: it replaces that one only. */
  amendOn: string | null;
  prefilled: string;
  /** Sign-off: the message ends with Signed-off-by (`--signoff`). It stays on for the folder
   *  until turned off, over commits. */
  signOff: boolean;
  /** The changelist Commit takes instead of the staged changes; null: the staged changes. */
  changelist: string | null;
}

// The commit message outlives the panel, so picking another commit and coming back keeps the
// draft. Each repository has its own: a message or Amend from one never lands in another.
const drafts = new Map<string, Draft>();
// It outlives the app too. Each folder has its own key, so a worktree keeps its own draft and
// two windows never write over each other's.
const DRAFT_KEY = "oxbow.commitDraft:";

function savedDraft(repo: string): Partial<Draft> {
  try {
    const saved = JSON.parse(localStorage.getItem(DRAFT_KEY + repo) ?? "null");
    return saved && typeof saved === "object" ? saved : {};
  } catch {
    return {};
  }
}

export function saveDraft(repo: string, draft: Draft) {
  try {
    if (!draft.summary.trim() && !draft.description.trim() && !draft.amend && !draft.signOff && !draft.changelist) localStorage.removeItem(DRAFT_KEY + repo);
    else localStorage.setItem(DRAFT_KEY + repo, JSON.stringify(draft));
  } catch {
    // Private mode or full storage: the draft lasts until the app quits.
  }
}

/** The commit message draft saved for the folder `repo`, as it was saved. */
export function savedMessage(repo: string): unknown {
  return drafts.get(repo) ?? savedDraft(repo);
}

/** The folder `from` was moved to `to`: its commit message goes along, unless `to` has one of
 *  its own. True when it went. */
export function moveMessage(from: string, to: string): boolean {
  const draft = draftFor(from);
  if (!draft.summary.trim() && !draft.description.trim() && !draft.amend) return false;
  const there = draftFor(to);
  if (there.summary.trim() || there.description.trim() || there.amend) return false;
  Object.assign(there, $state.snapshot(draft));
  saveDraft(to, there);
  drafts.delete(from);
  saveDraft(from, { summary: "", description: "", amend: false, amendOn: null, prefilled: "", signOff: false, changelist: null });
  return true;
}

export function draftFor(repo: string): Draft {
  const known = drafts.get(repo);
  if (known) return known;
  const saved = savedDraft(repo);
  const text = (value: unknown) => (typeof value === "string" ? value : "");
  const draft = $state<Draft>({
    summary: text(saved.summary),
    description: text(saved.description),
    amend: saved.amend === true,
    amendOn: typeof saved.amendOn === "string" ? saved.amendOn : null,
    prefilled: text(saved.prefilled),
    signOff: saved.signOff === true,
    changelist: typeof saved.changelist === "string" ? saved.changelist : null,
  });
  drafts.set(repo, draft);
  return draft;
}
