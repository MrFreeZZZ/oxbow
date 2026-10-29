// The form that opens a pull request (a merge request on GitLab) for a branch that has none, on
// the Pull Request screen: what it starts with (the branch's commits, CODEOWNERS, GitLab's own
// defaults and templates), what is picked in it, and the pull request it would open.

import { untrack } from "svelte";
import { api } from "./api";
import { github } from "./github.svelte";
import { wordsOf, type ForgeKind } from "./forge";
import type { BranchContext } from "./branches";
import type { GitLabCreateFacts, GitLabUser, History, PullSummary, Stack } from "./types";
import type { Pick } from "./PickChips.svelte";
import { defaultTemplate, squashStarts } from "./mrForm";
import { branchMergeRequest } from "./mergeRequest";
import type { NewPull, StackLine } from "./pr";

/** What the screen knows, read as it changes: give each as a getter. */
export interface DraftFacts {
  readonly branch: string;
  readonly forge: ForgeKind;
  /** The branch's pull request, when it has one: then there is no form. */
  readonly summary: PullSummary | null;
  readonly stack: Stack | null;
  /** The trunk's name without its remote, e.g. "main". */
  readonly trunk: string;
  /** The GitHub account signed in, by its login. */
  readonly me: string | null;
  readonly history: History;
  /** Where it goes: the remote, and the account that owns the repository on GitHub. */
  readonly place: { remote: string; owner: string | null } | null;
  readonly ctx: BranchContext;
}

export interface PullForm {
  branch: string;
  title: string;
  body: string;
  base: string;
  draft: boolean;
  reviewers: string[];
  adding: string;
  suggested: string[];
  /** GitLab: what else it opens with, and the account ids of the usernames picked. */
  assignee?: string | null;
  labels?: string[];
  removeSource?: boolean;
  squash?: boolean;
  template?: string | null;
  known?: Record<string, number>;
}

export class PullDraft {
  form = $state<PullForm | null>(null);
  /** GitLab's labels and settings for the form, and the templates on the trunk. */
  labFacts = $state<GitLabCreateFacts | null>(null);
  labFactsError = $state<string | null>(null);

  #facts: DraftFacts;
  /** A new form waits for GitLab's facts to take its defaults from. */
  #fresh = false;
  #factsAsked = false;
  /** GitLab's people found, by username: their ids, for sending. */
  #ids: Record<string, number> = {};

  constructor(facts: DraftFacts) {
    this.#facts = facts;
  }

  get #lab() {
    return this.#facts.forge === "gitlab";
  }

  /** The branch under this one in the stack, the natural base. */
  readonly below = $derived.by(() => {
    const { stack, branch } = this.#facts;
    if (!stack) return null;
    const i = stack.branches.findIndex((b) => b.name === branch);
    return i >= 0 && i + 1 < stack.branches.length ? stack.branches[i + 1].name : null;
  });
  readonly bases = $derived.by(() => [...(this.below ? [this.below] : []), this.#facts.trunk].filter((b, i, all) => all.indexOf(b) === i));

  /** Commits `branch` brings into `base`, from the stack (newest first). */
  commitsOver(base: string): number {
    const { stack, branch } = this.#facts;
    if (!stack) return 0;
    const tipOf = (name: string) => stack.commits.findIndex((c) => c.id === stack.branches.find((b) => b.name === name)?.at);
    const from = tipOf(branch);
    if (from < 0) return 0;
    const to = base === this.below ? tipOf(base) : stack.commits.length;
    return Math.max(0, (to < 0 ? stack.commits.length : to) - from);
  }

  readonly localRef = $derived.by(() => this.#facts.history.refs.find((r) => r.kind === "local" && r.name === this.#facts.branch) ?? null);
  /** The commits a push takes up first. */
  readonly toPush = $derived.by(() => {
    if (!this.localRef) return 0;
    const t = this.localRef.tracking;
    if (!t || t.gone) return this.commitsOver(this.form?.base ?? this.#facts.trunk) || 1;
    return t.ahead;
  });

  /** The form starts from the branch's own commits. Runs in an effect. */
  start() {
    const { summary, stack, branch } = this.#facts;
    if (summary || !stack || this.form?.branch === branch) return;
    const base = this.below ?? this.#facts.trunk;
    const own = stack.commits.filter((c) => c.branch === branch);
    // The newest commit says best where the branch ended up.
    const title = own[0]?.summary ?? branch;
    const text = own.length === 1 ? own[0].message.split("\n").slice(1).join("\n").trim() : "";
    this.form = { branch, title, body: text, base, draft: true, reviewers: [], adding: "", suggested: [] };
    if (this.#lab) {
      this.form = { ...this.form, assignee: null, labels: [], removeSource: true, squash: false, template: null, known: {} };
      const facts = untrack(() => this.labFacts);
      if (facts) this.#startFrom(facts);
      else this.#fresh = true;
    }
    this.#suggest(base);
  }

  /** GitLab's facts, once a form is up. Runs in an effect. */
  askFacts() {
    const stack = this.#facts.stack;
    if (!this.#lab || !this.form || !stack || this.labFacts || this.labFactsError || this.#factsAsked) return;
    const rev = stack.trunk;
    untrack(() => this.#loadLabFacts(rev));
  }

  async #loadLabFacts(rev: string) {
    this.#factsAsked = true;
    try {
      this.labFacts = await api.gitlabCreateFacts(rev);
      if (this.#fresh && this.form) this.#startFrom(this.labFacts);
      this.#fresh = false;
    } catch (err) {
      this.labFactsError = String(err);
    } finally {
      this.#factsAsked = false;
    }
  }

  /** The defaults GitLab gives a new merge request: the project's way with merged branches and
   *  squashing, and the Default template for the description. */
  #startFrom(facts: GitLabCreateFacts) {
    const form = this.form;
    if (!form) return;
    form.removeSource = facts.removeSourceBranch;
    form.squash = squashStarts(facts.squashOption);
    const start = defaultTemplate(facts.templates);
    if (start) {
      form.template = start.name;
      form.body = start.text;
    }
  }

  pickTemplate(name: string | null) {
    if (!this.form || !this.labFacts) return;
    this.form.template = name;
    this.form.body = this.labFacts.templates.find((t) => t.name === name)?.text ?? "";
  }

  async #suggest(base: string) {
    const name = this.#facts.branch;
    const ref = base === this.below ? base : (this.#facts.stack?.trunk ?? base);
    if (this.#lab) {
      const owners = await api.gitlabCodeOwners(ref, name).catch(() => [] as GitLabUser[]);
      const form = this.form;
      if (!form || form.branch !== name) return;
      const others = owners.filter((o) => o.username !== github.mrMe);
      form.known = { ...form.known, ...Object.fromEntries(others.map((o) => [o.username, o.id])) };
      form.suggested = others.map((o) => o.username);
      form.reviewers = [...form.suggested];
      return;
    }
    const owners = await api.codeOwners(ref, name).catch(() => [] as string[]);
    const form = this.form;
    if (!form || form.branch !== name) return;
    form.suggested = owners.filter((o) => o !== this.#facts.me);
    form.reviewers = [...form.suggested];
  }

  /** GitLab's people for a box of the form: name, username and their id kept for sending. */
  async findPeople(text: string): Promise<Pick[]> {
    if (!text.trim()) return [];
    const people = await api.gitlabPeople(text);
    for (const p of people) this.#ids[p.username] = p.id;
    if (this.form) this.form.known = { ...this.form.known, ...Object.fromEntries(people.map((p) => [p.username, p.id])) };
    return people.map((p) => ({ key: p.username, label: p.username, sub: p.name }));
  }

  /** The GitLab account id of a username found or picked here. */
  idOf(username: string): number | undefined {
    return this.#ids[username] ?? this.form?.known?.[username];
  }

  labelPicks(names: string[]): Pick[] {
    return names.map((name) => ({ key: name, label: name, color: this.labFacts?.labels.find((l) => l.name === name)?.color }));
  }

  pickBase(base: string) {
    if (!this.form || this.form.base === base) return;
    this.form.base = base;
    this.#suggest(base);
  }

  addReviewer() {
    if (!this.form) return;
    const login = this.form.adding.trim().replace(/^@/, "");
    if (login && !this.form.reviewers.includes(login)) this.form.reviewers = [...this.form.reviewers, login];
    this.form.adding = "";
  }

  readonly stackLines = $derived.by((): StackLine[] | null => {
    const { stack, branch, forge } = this.#facts;
    if (!stack || stack.branches.length < 2) return null;
    const w = wordsOf(forge);
    const lines = [...stack.branches].reverse().map((b) => {
      const p = github.pullOf(b.name, forge);
      return { branch: b.name, number: b.name === branch ? `${w.mark}$NUMBER` : p ? `${w.mark}${p.number}` : null, merged: p?.state === "merged" };
    });
    return lines.some((l) => l.branch !== branch && l.number) ? lines : null;
  });
  /** The stack's other open pull requests, oldest first. */
  readonly others = $derived.by(() => {
    const { stack, branch, forge } = this.#facts;
    return stack ? [...stack.branches].reverse().map((b) => github.openPullOf(b.name, forge)).filter((p): p is PullSummary => !!p && p.head !== branch) : [];
  });

  /** The pull request the form would open, with the push it needs first. */
  readonly newPull = $derived.by((): NewPull | null => {
    const form = this.form;
    const { place, branch, ctx } = this.#facts;
    if (!form || !place) return null;
    const lab = this.#lab;
    const localRef = this.localRef;
    const t = localRef?.tracking;
    const remote = t && !t.gone ? t.remote : place.remote;
    const needsPush = !t || t.gone || t.ahead > 0;
    const known = form.known ?? {};
    const project = github.mrProject;
    const ref = localRef;
    const gitlab =
      lab && project && ref
        ? {
            create: {
              reviewerIds: form.reviewers.flatMap((r) => (known[r] ? [known[r]] : [])),
              assigneeIds: form.assignee && known[form.assignee] ? [known[form.assignee]] : [],
              labels: form.labels ?? [],
              removeSourceBranch: !!form.removeSource,
              squash: !!form.squash,
            },
            assignee: form.assignee ?? null,
            fallback: () => branchMergeRequest(ctx, ref, project),
          }
        : null;
    // A merge request needs its local branch: that's what goes up, and what the push path asks for.
    if (lab && !gitlab) return null;
    return {
      branch,
      base: form.base,
      title: form.title,
      body: form.body,
      draft: form.draft,
      reviewers: form.reviewers,
      push: needsPush ? { kind: "push", remote, branch, upstream: t && !t.gone ? t.branch : branch, setUpstream: !t || t.gone, force: false, lease: null, noVerify: false } : null,
      stack: this.stackLines,
      others: this.others,
      commits: this.commitsOver(form.base),
      gitlab,
    };
  });
}
