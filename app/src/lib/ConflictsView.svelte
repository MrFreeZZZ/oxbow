<script lang="ts">
  // The Conflicts screen: conflicted files on the left; for the open file, both sides on top
  // (named by branch, never "ours" and "theirs"), with the version they started from when asked,
  // and the result underneath, where each conflict gets a side, both, or lines typed by hand,
  // or the whole file is written as text, before the file is marked resolved. All of it is saved
  // as a draft as the user goes (lib/drafts.ts), so quitting Oxbow keeps it.

  import TipCard from "./TipCard.svelte";
  import EditBox from "./EditBox.svelte";
  import { onMount, untrack } from "svelte";
  import { api } from "./api";
  import { confirm, type Request } from "./confirm.svelte";
  import { kept } from "./kept";
  import { prefs } from "./prefs.svelte";
  import { hasWork, makeDraft, readDraft, takeDraft, type Draft, type DraftState, type DraftText, type OldDraft } from "./drafts";
  import {
    conflictSteps,
    joinLines,
    KEPT_CONFLICTS,
    kindOf,
    linesOf,
    markOf,
    fileText,
    markerRoles,
    markerStarts,
    openMarkers,
    resultOf,
    splitLines,
    stepIndex,
    strayMarkers,
    toResolution,
    type Choice,
    type Decision,
    type Sides,
  } from "./conflicts";
  import type { Chunk, ConflictFile, ConflictSide, History, MergeTool, Operation, WorkingTree } from "./types";
  import { NO_BRANCH_COLOR, plate, relativeTime, shortId, splitPath, tint } from "./format";

  let {
    history,
    op,
    colorOf,
    version,
    run,
    refresh,
  }: {
    history: History;
    op: Operation;
    colorOf: (name: string) => number;
    /** Goes up on every reload, so the lists reload too. */
    version: number;
    run: (request: Request) => void;
    /** Reload the window, after the merge tool changed the repository. */
    refresh: () => void;
  } = $props();

  type Conflict = Extract<Chunk, { kind: "conflict" }>;
  /** A file written as text: what it is now, what it started as, to tell an edit, and how many
   *  of its lines are what is left of a conflict whose markers were partly deleted. */
  type Text = DraftText;
  /** What the screen had going, kept while the user is on another screen; `stages` says which
   *  versions of each file its decisions were made for. */
  type Saved = DraftState & { step: string };

  let tree = $state<WorkingTree | null>(null);
  let current = $state<string | null>(null);
  let file = $state<ConflictFile | null>(null);
  let error = $state<string | null>(null);
  /** Decisions per file, one per conflict, kept while the user moves between files. */
  // A Map, as a conflicted file may be called `constructor` or `__proto__`.
  let choices = $state.raw(new Map<string, Decision[]>());
  /** Files being written as text. */
  let texts = $state.raw(new Map<string, Text>());
  /** The versions of each file in the index that its decisions and text were made for. */
  let stages = new Map<string, string>();
  /** Every file that was conflicted in this step, so resolved ones still show as done. */
  let seen = $state<string[]>([]);
  /** Each file as its decisions or text would write it, for a draft that stops fitting it. */
  let results = new Map<string, string>();
  /** Drafts made for other versions of a file, kept to compare with it. */
  let old = $state.raw(new Map<string, OldDraft>());
  /** The open file's old draft is shown in place of the result. */
  let comparing = $state(false);
  /** When the decisions on screen were last saved, or came back from a draft. */
  let draftAt = 0;
  /** What saving the draft last came to, shown apart from Mark Resolved. */
  let draftNote = $state<{ kind: "saved"; at: number } | { kind: "failed"; why: string } | null>(null);
  /** The operation, its commit, and the commit checked out, that the decisions belong to. */
  let step = $state("");
  /** The step whose draft is being read from disk. */
  let reading = "";
  /** The conflict last stepped to in the open file; -1 before the first step. */
  let at = $state(-1);
  /** A conflict just turned into lines to type: its box takes the focus. */
  let fresh = $state<number | null>(null);
  /** Leaving the text asks first, as typed text would be dropped. */
  let leaving = $state(false);
  /** The merge tool git opens; undefined until known. */
  let tool = $state<MergeTool | null | undefined>(undefined);
  /** The file open in the merge tool. */
  let waiting = $state<string | null>(null);
  let work = $state<HTMLElement>();
  let textRows = $state<HTMLElement>();

  // On top the side being brought in, on the right of it the user's own work.
  const leftSide = $derived<ConflictSide>(op.yours === "ours" ? "theirs" : "ours");
  const rightSide = $derived<ConflictSide>(op.yours);
  const labelOf = (side: ConflictSide) => (side === "ours" ? op.oursLabel : op.theirsLabel);
  const leftLabel = $derived(labelOf(leftSide));
  const rightLabel = $derived(labelOf(rightSide));
  const sides = $derived<Sides>({ left: leftSide, right: rightSide, leftLabel, rightLabel });
  const isRef = (name: string) => history.refs.some((r) => r.name === name);
  const leftColor = $derived(isRef(leftLabel) ? colorOf(leftLabel) : NO_BRANCH_COLOR);
  const rightColor = $derived(op.branch ? colorOf(op.branch) : NO_BRANCH_COLOR);
  const colorFor = (c: Choice) => (c === "left" ? leftColor : rightColor);

  const leftNote = $derived(
    op.kind === "merge"
      ? `Incoming · ${op.incomingCount} new ${op.incomingCount === 1 ? "commit" : "commits"} on ${leftLabel}`
      : op.kind === "rebase"
        ? `Upstream · the new base`
        : op.kind === "cherryPick"
          ? `Picked commit${op.commit ? ` · ${op.commit.summary}` : ""}`
          : op.kind === "revert"
            ? "The revert"
            : op.kind === "stashApply"
              ? `The stash${op.message ? ` · ${op.message}` : ""}`
              : op.incoming
              ? `Incoming · all changes of ${op.incoming}, as one commit`
              : "Squashed changes",
  );
  const rightNote = $derived(
    op.kind === "rebase"
      ? `Your commit${op.commit ? ` · ${shortId(op.commit.id)} ${op.commit.summary}` : ""}`
      : `Yours · ${rightLabel} is checked out`,
  );

  const conflicted = $derived(tree?.conflicted.map((f) => f.path) ?? []);
  const resolved = $derived(seen.filter((p) => !conflicted.includes(p)));
  const automatic = $derived((tree?.staged ?? []).map((f) => f.path).filter((p) => !seen.includes(p)));

  async function reload() {
    try {
      const next = await api.workingTree();
      tree = next;
      error = null;
      const key = `${op.kind}:${op.commit?.id ?? ""}:${history.head.commit ?? ""}`;
      const paths = next.conflicted.map((f) => f.path);
      if (key !== step) {
        if (key === reading) return;
        reading = key;
        // Back on the screen in the same step: the decisions are still there. Otherwise they
        // come from the draft on disk, as after Oxbow was quit.
        const saved = kept.get<Saved>(KEPT_CONFLICTS);
        const draft = saved?.step === key ? null : await loadDraft();
        if (reading !== key) return;
        reading = "";
        const state = saved?.step === key ? { ...saved, seen: [...saved.seen, ...paths.filter((p) => !saved.seen.includes(p))] } : takeDraft(draft, key, paths);
        choices = state.choices;
        texts = state.texts;
        stages = state.stages;
        results = state.results;
        old = state.old;
        seen = state.seen;
        step = key;
        if (draft?.step === key) draftAt = draft.savedAt;
        if (draft) tellRestored(draft, state);
      } else {
        seen = [...seen, ...paths.filter((p) => !seen.includes(p))];
      }
      if (!current || !paths.includes(current)) current = paths[0] ?? null;
      await open(current, true);
    } catch (err) {
      error = String(err);
    }
  }

  async function loadDraft(): Promise<Draft | null> {
    try {
      return readDraft(await api.conflictDraft());
    } catch (err) {
      draftNote = { kind: "failed", why: String(err) };
      return null;
    }
  }

  /** Say what came back from the draft, as the decisions on screen don't say they are old. */
  function tellRestored(draft: Draft, state: DraftState) {
    const back = conflictedNow().filter((p) => hasWork(state.choices.get(p), state.texts.get(p))).length;
    const when = since(draft.savedAt);
    if (back) confirm.say(`Your draft from ${when} is back: ${back} ${back === 1 ? "file" : "files"} as you left ${back === 1 ? "it" : "them"}.`, "info");
  }

  /** "2 min ago", "yesterday 17:05": when a draft was saved, inside a sentence. */
  const since = (ms: number) => relativeTime(ms / 1000).replace(/^(Just|Yesterday)/, (word) => word.toLowerCase());

  const conflictedNow = () => tree?.conflicted.map((f) => f.path) ?? [];

  async function open(path: string | null, again = false) {
    if (!again || path !== current) {
      at = -1;
      leaving = false;
      fresh = null;
      comparing = false;
    }
    current = path;
    if (!path) {
      file = null;
      return;
    }
    try {
      const next = await api.conflictFile(path);
      if (current !== path) return;
      // Decisions made for other versions of the file, as after an abort and a new merge, start
      // over: the same number of conflicts is no sign they still fit. What they made is kept,
      // to compare.
      const was = stages.get(path);
      stages.set(path, next.stages);
      if (was !== undefined && was !== next.stages) {
        if (hasWork(choices.get(path), texts.get(path))) {
          old = new Map(old).set(path, { path, result: results.get(path) ?? texts.get(path)?.text ?? "", savedAt: draftAt || Date.now() });
        }
        results.delete(path);
        choices = new Map(choices);
        choices.delete(path);
        if (texts.has(path)) {
          texts = new Map(texts);
          texts.delete(path);
        }
      }
      file = next;
      const count = next.chunks.filter((c) => c.kind === "conflict").length;
      if (choices.get(path)?.length !== count) choices = new Map(choices).set(path, Array(count).fill(null));
    } catch (err) {
      error = String(err);
    }
  }

  $effect(() => {
    version;
    op;
    untrack(() => reload());
  });

  /** The save waiting for typing to pause. */
  let pending: (() => void) | null = null;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  /** Saves one after another, so an older one never lands over a newer. */
  let saving = Promise.resolve();

  $effect(() => {
    if (!step || !tree) return;
    // The open file's result, when its decisions were made for the versions on screen.
    if (file && current === file.path && stages.get(current) === file.stages) results.set(current, resultOf(file, picked, asText, sides));
    const state: Saved = { step, choices, texts, stages, results, old, seen };
    kept.set(KEPT_CONFLICTS, state);
    const paths = conflicted;
    pending = () => {
      saving = saving.then(() => saveDraft(state, paths));
    };
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 300);
  });

  function flush() {
    clearTimeout(saveTimer);
    const save = pending;
    pending = null;
    save?.();
  }

  async function saveDraft(state: Saved, paths: string[]) {
    const at = Date.now();
    const draft = makeDraft(state.step, state, paths, at);
    try {
      if (draft) await api.saveConflictDraft(draft);
      else await api.dropConflictDraft();
      draftAt = at;
      draftNote = draft ? { kind: "saved", at } : null;
    } catch (err) {
      draftNote = { kind: "failed", why: String(err) };
    }
  }

  onMount(() => {
    api
      .mergeTool()
      .then((found) => (tool = found))
      .catch(() => (tool = null));
    conflictSteps.step = stepConflict;
    window.addEventListener("beforeunload", flush);
    return () => {
      if (conflictSteps.step === stepConflict) conflictSteps.step = null;
      window.removeEventListener("beforeunload", flush);
      flush();
    };
  });

  const picked = $derived(current ? (choices.get(current) ?? []) : []);
  const asText = $derived(current ? (texts.get(current) ?? null) : null);
  const conflicts = $derived((file?.chunks ?? []).filter((c): c is Conflict => c.kind === "conflict"));
  const open_ = $derived(asText ? openMarkers(asText.text) : picked.filter((c) => c === null).length);
  const stray = $derived(asText?.stray ?? 0);
  const lineByLine = $derived(!!file && file.chunks.length > 0);
  const kind = $derived(file ? kindOf(file, labelOf) : null);
  const showBase = $derived(prefs.get("oxbow.conflicts.showBase") && !!file?.base && lineByLine);

  function decide(k: number, decision: Decision) {
    if (!current) return;
    const list = [...(choices.get(current) ?? [])];
    list[k] = decision;
    choices = new Map(choices).set(current, list);
  }

  /** Turn a conflict into lines to type, starting from what it holds: both sides while open. */
  function edit(k: number) {
    const chunk = conflicts[k];
    if (!chunk) return;
    const lines = linesOf(chunk, picked[k] ?? null, sides) ?? [...chunk[leftSide], ...chunk[rightSide]];
    fresh = k;
    decide(k, { lines: [...lines] });
  }

  function startText() {
    if (!current || !file?.text) return;
    const text = fileText(file.chunks, picked, sides, showBase, file.openEnd);
    texts = new Map(texts).set(current, { text, start: text, stray: strayMarkers(text, file.chunks) });
    at = -1;
  }

  function stopText() {
    if (!asText) return;
    if (asText.text !== asText.start && !leaving) {
      leaving = true;
      return;
    }
    dropText();
  }

  function dropText() {
    if (!current) return;
    const next = new Map(texts);
    next.delete(current);
    texts = next;
    leaving = false;
    at = -1;
  }

  function typedText(value: string) {
    if (!current || !asText || !file) return;
    texts = new Map(texts).set(current, { ...asText, text: value, stray: strayMarkers(value, file.chunks) });
  }

  /** Go to the next or previous conflict (⌥F5, ⇧⌥F5), in every pane at once. */
  function stepConflict(by: 1 | -1): boolean {
    const starts = asText ? markerStarts(asText.text) : null;
    const count = starts ? starts.length : conflicts.length;
    if (!count || !work) return false;
    at = at < 0 || at >= count ? (by > 0 ? 0 : count - 1) : stepIndex(at, by, count);
    const targets = starts
      ? [textRows?.children[starts[at]] as HTMLElement | undefined]
      : [...work.querySelectorAll<HTMLElement>(`[data-conflict="${at}"]`)];
    for (const target of targets) {
      const scroller = target?.closest<HTMLElement>(".code");
      if (!target || !scroller) continue;
      scroller.scrollTop += target.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 8;
    }
    return true;
  }

  const stepCount = $derived(asText ? markerStarts(asText.text).length : conflicts.length);

  /** What each side looks like around the conflicts: one line of context, the rest folded. */
  type SideItem = { kind: "line"; no: number; text: string; k: number | null } | { kind: "fold"; count: number } | { kind: "head"; k: number } | { kind: "empty"; k: number };

  function sideItems(chunks: Chunk[], side: ConflictSide | "base"): SideItem[] {
    const items: SideItem[] = [];
    let no = 1;
    let k = 0;
    chunks.forEach((chunk, ci) => {
      if (chunk.kind === "same") {
        const n = chunk.lines.length;
        let folded = 0;
        chunk.lines.forEach((text, i) => {
          const keep = (ci > 0 && i === 0) || (ci < chunks.length - 1 && i === n - 1);
          if (!keep) {
            folded++;
            return;
          }
          if (folded) items.push({ kind: "fold", count: folded });
          folded = 0;
          items.push({ kind: "line", no: no + i, text, k: null });
        });
        if (folded) items.push({ kind: "fold", count: folded });
        no += n;
      } else {
        items.push({ kind: "head", k });
        const lines = chunk[side];
        if (!lines.length) items.push({ kind: "empty", k });
        for (const text of lines) items.push({ kind: "line", no: no++, text, k });
        k++;
      }
    });
    return dedent(items);
  }

  /** Narrow panes drop the indentation every shown line shares, like the design. */
  function dedent(items: SideItem[]): SideItem[] {
    const indents = items.flatMap((i) => (i.kind === "line" && i.text.trim() ? [/^[ \t]*/.exec(i.text)![0].length] : []));
    const cut = indents.length ? Math.min(...indents) : 0;
    return cut ? items.map((i) => (i.kind === "line" ? { ...i, text: i.text.slice(cut) } : i)) : items;
  }

  /** The whole result: agreed lines, picked lines tinted by where they came from, lines typed by
   *  hand, open conflicts. */
  type ResultItem =
    | { kind: "line"; no: number; text: string; from: Choice | null; k: number | null; edge: "top" | "bottom" | "both" | null }
    | { kind: "open"; k: number }
    | { kind: "bar"; k: number; choice: Choice }
    | { kind: "typed"; k: number; no: number; value: string };

  const resultItems = $derived.by((): ResultItem[] => {
    if (!file) return [];
    const items: ResultItem[] = [];
    let no = 1;
    let k = 0;
    for (const chunk of file.chunks) {
      if (chunk.kind === "same") {
        for (const text of chunk.lines) items.push({ kind: "line", no: no++, text, from: null, k: null, edge: null });
        continue;
      }
      const choice = picked[k] ?? null;
      if (!choice) items.push({ kind: "open", k });
      else if (typeof choice === "object") {
        items.push({ kind: "typed", k, no, value: joinLines(choice.lines) });
        no += choice.lines.length;
      } else {
        items.push({ kind: "bar", k, choice });
        const left = chunk[leftSide].map((text) => ({ text, from: "left" as Choice }));
        const right = chunk[rightSide].map((text) => ({ text, from: "right" as Choice }));
        const lines = choice === "left" ? left : choice === "right" ? right : [...left, ...right];
        lines.forEach((l, i) => {
          const top = i === 0 || lines[i - 1].from !== l.from;
          const bottom = i === lines.length - 1 || lines[i + 1].from !== l.from;
          items.push({ kind: "line", no: no++, text: l.text, from: l.from, k, edge: top && bottom ? "both" : top ? "top" : bottom ? "bottom" : null });
        });
      }
      k++;
    }
    return items;
  });
  /** Typed boxes keep their place by conflict, so typing in one never lands in another. */
  const itemKey = (item: ResultItem, i: number) => (item.kind === "typed" ? `typed${item.k}` : `${item.kind}${i}`);

  const leftItems = $derived(file ? sideItems(file.chunks, leftSide) : []);
  const rightItems = $derived(file ? sideItems(file.chunks, rightSide) : []);
  const baseItems = $derived(file && showBase ? sideItems(file.chunks, "base") : []);

  /** Tints of the text: git's markers orange, each side's lines in its branch color. */
  const roles = $derived(asText ? markerRoles(asText.text) : []);
  const textTint = (i: number, line: string) => {
    const role = roles[i];
    // What is left of a block shows orange too, while the text has some.
    return role === "marker" || (role === null && stray > 0 && markOf(line))
      ? "var(--orange-soft)"
      : role === "left"
        ? tint(leftColor, "soft")
        : role === "right"
          ? tint(rightColor, "soft")
          : role === "base"
            ? "var(--field)"
            : undefined;
  };

  const conflictsIn = (path: string) => {
    if (texts.has(path)) return openMarkers(texts.get(path)!.text);
    const list = choices.get(path);
    // No conflicts line by line: the file is taken whole from one side.
    return list?.length ? list.filter((c) => c === null).length : null;
  };

  function markResolved() {
    if (!current || !file || open_) return;
    const name = splitPath(current).name;
    const request = {
      title: `Mark ${name} as resolved?`,
      icon: "stage",
      button: "Mark Resolved",
      status: `Resolving ${name}…`,
      done: `${name} is resolved.`,
    } as const;
    if (asText) {
      run({
        ...request,
        body: ["Oxbow writes the text you edited into ", { code: current }, ". Then git stages the file, which marks the conflict resolved."],
        ...(stray
          ? {
              title: `Mark ${name} as resolved with markers left in it?`,
              button: "Mark Resolved Anyway",
              danger: true,
              always: true,
              warning: {
                title: `${stray} ${stray === 1 ? "line looks" : "lines look"} like what is left of a conflict`,
                body: [
                  "A conflict marker (",
                  { code: "<<<<<<<" },
                  ", ",
                  { code: "=======" },
                  " or ",
                  { code: ">>>>>>>" },
                  ") is still in the text without the rest of its block. Those lines would be committed as they are.",
                ],
              },
            }
          : {}),
        action: { kind: "resolve", path: current, picks: [], text: asText.text, stages: file.stages },
      });
      return;
    }
    const list = picked as Exclude<Decision, null>[];
    const words = list.map((c) => (typeof c === "object" ? "lines you typed" : c === "both" ? "both sides" : c === "left" ? leftLabel : rightLabel));
    const same = words.every((w) => w === words[0]);
    run({
      ...request,
      body: [
        "Oxbow writes your choices into ",
        { code: current },
        same && words.length > 1 ? `: ${words[0]} in all ${words.length} conflicts. ` : ". ",
        "Then git stages the file, which marks the conflict resolved.",
      ],
      action: { kind: "resolve", path: current, picks: list.map((d) => toResolution(d, sides)), stages: file.stages },
    });
  }

  /** One side's whole file, for files that can't be merged line by line. */
  function takeFile(side: ConflictSide) {
    if (!current || !file) return;
    const name = splitPath(current).name;
    const label = labelOf(side);
    const exists = side === "ours" ? file.ours : file.theirs;
    run({
      title: exists ? `Use ${label}’s ${name}?` : `Delete ${name}, as on ${label}?`,
      body: exists
        ? ["The whole file comes from ", { branch: label, color: side === leftSide ? leftColor : rightColor }, "; the other side’s changes to it are dropped."]
        : [{ branch: label, color: side === leftSide ? leftColor : rightColor }, " deleted ", { code: current }, ". Taking that side deletes it here too."],
      icon: exists ? "stage" : "discard",
      danger: !exists,
      button: exists ? `Use ${label}’s Version` : "Delete File",
      status: `Resolving ${name}…`,
      done: `${name} is resolved.`,
      action: { kind: "takeFile", path: current, side },
    });
  }

  /** Why the open file can't be merged line by line. */
  const wholeOnly = $derived.by(() => {
    if (!file || lineByLine) return null;
    if (file.binary) return "This is a binary file, so it can’t be merged line by line. Pick one side’s version.";
    if (file.link) return "This is a symbolic link or a submodule, so it can’t be merged line by line. Pick one side’s version.";
    if (!file.ours || !file.theirs) {
      const gone = !file.ours ? labelOf("ours") : labelOf("theirs");
      const kept = !file.ours ? labelOf("theirs") : labelOf("ours");
      return `${gone} deleted this file, while ${kept} changed it. Keep ${kept}’s version, or delete it.`;
    }
    return "Both sides changed this file in a way git can’t show line by line. Pick one side’s version.";
  });

  /** Why the merge tool can't open the file, or null when it can. */
  const toolWhy = $derived.by(() => {
    if (tool === undefined) return "Looking for the merge tool…";
    if (!tool) return "No merge tool is set. Set one with: git config --global merge.tool <name>";
    if (tool.terminal) return `${tool.name} runs in a terminal. Set merge.guitool to an app with a window.`;
    if (!file || !lineByLine) return "A merge tool needs both sides’ text of the file.";
    if (waiting && waiting !== current) return `${splitPath(waiting).name} is still open in ${tool.label}.`;
    return null;
  });
  const toolLabel = $derived(tool && !tool.terminal ? tool.label : "Merge Tool");

  /** Open the file in the merge tool; git waits for it to quit, then the screen reloads. */
  async function openTool() {
    if (!current || !tool || toolWhy || waiting) return;
    const path = current;
    const name = splitPath(path).name;
    waiting = path;
    try {
      const done = await api.openMergeTool(path);
      confirm.say(done ? `${name} is resolved in ${tool.label}.` : `${tool.label} closed. ${name} still has conflicts.`, done ? "done" : "info");
    } catch (err) {
      confirm.fail(err);
    } finally {
      waiting = null;
      refresh();
      await reload();
    }
  }

  const textWhy = $derived(file && !file.text ? "This file isn’t UTF-8 text, so it can’t be typed in here. Pick sides, or use the merge tool." : null);

  const oldDraft = $derived(current ? (old.get(current) ?? null) : null);
  /** The old draft's lines; its last line break ends the last line, it doesn't add one. */
  const oldLines = $derived(oldDraft && oldDraft.result !== "" ? oldDraft.result.replace(/\n$/, "").split("\n") : []);
  const oldWhen = $derived(oldDraft ? since(oldDraft.savedAt) : "");
  /** Why the old draft can't become the text to edit, or null when it can. */
  const useWhy = $derived(textWhy ?? (asText && asText.text !== asText.start ? "Leave Edit as Text first: the text you typed would be replaced" : null));

  /** Edit the old draft's text as the file's result, from where it was left. */
  function useOld() {
    if (!current || !file || !oldDraft || useWhy) return;
    // Started from the file as it is, so leaving the text asks first.
    const start = fileText(file.chunks, picked, sides, showBase, file.openEnd);
    texts = new Map(texts).set(current, { text: oldDraft.result, start, stray: strayMarkers(oldDraft.result, file.chunks) });
    dropOld();
  }

  function dropOld() {
    if (!current) return;
    const next = new Map(old);
    next.delete(current);
    old = next;
    comparing = false;
  }

  const radius = (edge: string | null) => (edge === "both" ? "9px" : edge === "top" ? "9px 9px 0 0" : edge === "bottom" ? "0 0 9px 9px" : "0");
</script>

{#snippet sidePane(items: SideItem[], label: string, note: string, color: number, choice: Choice | null)}
  <section class="pane" aria-label="{label}’s version">
    <header class="pane-head">
      <span class="cap" style:background={tint(color, "label")} style:color={plate(color)}>{label}</span>
      <span class="ellipsis note">{note}</span>
    </header>
    <div class="code side mono selectable">
      {#each items as item, i (i)}
        {#if item.kind === "head"}
          {@const decision = picked[item.k] ?? null}
          {@const chosen = choice !== null && (decision === choice || decision === "both")}
          <div class="chead" class:current={at === item.k && !asText} data-conflict={item.k} style:background={tint(color, "bar")}>
            <span style:color={plate(color)}>Conflict {item.k + 1}</span>
            <span class="spacer"></span>
            {#if choice === null || asText}
              <!-- The base and the text: nothing to take from here. -->
            {:else if chosen}
              <span class="in" style:color={plate(color)}>✓ In result</span>
            {:else if decision !== null && typeof decision === "object"}
              <span class="in muted">Typed in the result</span>
            {:else}
              <button class="take" onclick={() => decide(item.k, choice)}>Take {label}</button>
            {/if}
          </div>
        {:else if item.kind === "empty"}
          <div class="line inblock" style:background={tint(color, "soft")}><span class="no"></span><span class="text gone">Nothing here: {choice === null ? "these lines weren’t there" : `${label} removed these lines`}</span></div>
        {:else if item.kind === "fold"}
          <div class="fold">⋯ {item.count} unchanged {item.count === 1 ? "line" : "lines"}</div>
        {:else}
          <div class="line" class:inblock={item.k !== null} style:background={item.k !== null ? tint(color, "soft") : undefined}>
            <span class="no">{item.no}</span><span class="text">{item.text}</span>
          </div>
        {/if}
      {/each}
    </div>
  </section>
{/snippet}

<div class="conflicts">
  <aside class="files" aria-label="Conflicted files">
    <div class="ghead">
      <span>Conflicts</span>
      {#if step}<span class="count">{resolved.length} of {seen.length} {seen.length === 1 ? "file" : "files"} resolved</span>{/if}
    </div>
    <div class="track"><span class="fill" style:width="{seen.length ? (resolved.length / seen.length) * 100 : 0}%"></span></div>
    <div class="list">
      {#each conflicted as path (path)}
        {@const p = splitPath(path)}
        {@const left = conflictsIn(path)}
        {@const strays = texts.get(path)?.stray ?? 0}
        {@const what = current === path && kind && kind.short !== "Text" ? kind.short : null}
        <button class="file" class:on={current === path} onclick={() => open(path)}>
          <span class="mark warn">!</span>
          <span class="fwords">
            <span class="ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            <span class="sub ellipsis" class:orange={left !== 0 || strays > 0} class:green={left === 0 && !strays}>
              {left === null ? (what ?? "Conflicted") : `${what ? `${what} · ` : ""}${left === 0 && strays ? `${strays} stray ${strays === 1 ? "marker" : "markers"}` : left === 0 ? "Ready to mark resolved" : `${left} ${left === 1 ? "conflict" : "conflicts"} left`}${texts.has(path) ? " · as text" : ""}`}{old.has(path) ? " · old draft" : ""}
            </span>
          </span>
        </button>
      {/each}
      {#each resolved as path (path)}
        {@const p = splitPath(path)}
        <div class="file done">
          <span class="mark ok">✓</span>
          <span class="fwords">
            <span class="ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            <span class="sub green">Resolved · staged</span>
          </span>
        </div>
      {/each}
      {#if automatic.length}
        <div class="ghead sub-head"><span>Merged automatically</span></div>
        {#each automatic as path (path)}
          {@const p = splitPath(path)}
          <div class="auto"><span class="check">✓</span><span class="ellipsis"><span class="dir">{p.dir}</span>{p.name}</span></div>
        {/each}
      {/if}
    </div>
    {#if op.kind === "rebase"}<div class="tip"><TipCard id="rebaseConflict" /></div>{/if}
    <p class="hint">
      {#if op.kind === "stashApply"}
        Nothing here is final until you finish. Undo Apply takes back what the stash brought and keeps the stash.
      {:else}
        Nothing here is final until you {op.kind === "merge" || op.kind === "squash" ? "commit" : "continue"}. Abort puts everything back as it was before the {op.kind === "cherryPick" ? "cherry-pick" : op.kind}.
      {/if}
      Your choices are kept as a draft until then, even if Oxbow quits.
    </p>
  </aside>

  <div class="work" bind:this={work}>
    {#if error}
      <p class="empty error">{error}</p>
    {:else if !step}
      <!-- Reading the files and the draft. -->
    {:else if !current}
      <div class="empty">
        <span class="big">✓</span>
        <span class="title">Every conflict is resolved</span>
        <span>{op.kind === "merge" || op.kind === "squash" ? "Commit the merge" : op.kind === "stashApply" ? "Finish" : "Continue"} with the button in the banner above.</span>
      </div>
    {:else if file}
      {@const p = splitPath(file.path)}
      <div class="filebar">
        <span class="fname ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
        {#if kind}<span class="kind ellipsis" title={kind.long}>{kind.long}</span>{/if}
        <span class="spacer"></span>
        {#if waiting === current}
          <span class="waitnote">Save and quit {toolLabel} to come back here.</span>
        {/if}
        {#if lineByLine}
          <button
            class="chip"
            class:on={showBase}
            aria-pressed={showBase}
            disabled={!file.base}
            title={file.base ? "Show the version both sides started from" : "Both sides added this file, so there is no version they started from"}
            onclick={() => prefs.set("oxbow.conflicts.showBase", !prefs.get("oxbow.conflicts.showBase"))}
          >
            Common Ancestor
          </button>
        {/if}
        <button class="button small" disabled={!!toolWhy || !!waiting} title={toolWhy ?? `Git writes the file with conflict markers, opens it in ${toolLabel}, and stages it once you save and quit`} onclick={openTool}>
          {waiting === current ? `Waiting for ${toolLabel}…` : `Open in ${toolLabel}`}
        </button>
      </div>
      {#if wholeOnly}
        <div class="empty">
          <span class="explain">{wholeOnly}</span>
          <div class="buttons">
            <button class="button" onclick={() => takeFile(leftSide)}>{(leftSide === "ours" ? file.ours : file.theirs) ? `Use ${leftLabel}’s Version` : `Delete, as on ${leftLabel}`}</button>
            <button class="button" onclick={() => takeFile(rightSide)}>{(rightSide === "ours" ? file.ours : file.theirs) ? `Use ${rightLabel}’s Version` : `Delete, as on ${rightLabel}`}</button>
          </div>
        </div>
      {:else}
        {#key current}
          <div class="sides">
            {#if showBase}
              {@render sidePane(baseItems, "Common ancestor", "Before both changes", NO_BRANCH_COLOR, null)}
            {/if}
            {@render sidePane(leftItems, leftLabel, leftNote, leftColor, "left")}
            {@render sidePane(rightItems, rightLabel, rightNote, rightColor, "right")}
          </div>
          <section class="pane result" aria-label="Result">
            <header class="pane-head">
              <b>Result</b>
              <span class="stepper">
                <button class="step" aria-label="Previous conflict" title="Previous conflict (⇧⌥F5)" disabled={!stepCount} onclick={() => stepConflict(-1)}>
                  <svg class="icon" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>
                </button>
                <button class="step" aria-label="Next conflict" title="Next conflict (⌥F5)" disabled={!stepCount} onclick={() => stepConflict(1)}>
                  <svg class="icon" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
                </button>
                <span class="small note">
                  {#if at >= 0 && at < stepCount}Conflict {at + 1} of {stepCount}{:else}{stepCount} {stepCount === 1 ? "conflict" : "conflicts"}{/if}
                </span>
              </span>
              {#if open_}
                <span class="orange small">· {open_} left</span>
              {:else if stray}
                <span class="orange small" title="Lines that look like conflict markers, without the rest of their block">· {stray} stray {stray === 1 ? "marker" : "markers"}</span>
              {:else}
                <span class="green small">· All decided</span>
              {/if}
              {#if draftNote?.kind === "failed"}
                <span class="orange small" title={draftNote.why}>· Draft not saved</span>
              {:else if draftNote && hasWork(picked, asText)}
                <span class="saved small" title="Your choices and text are saved in Oxbow’s own data, not in the repository, so quitting Oxbow keeps them. Mark Resolved writes them into the file.">· Draft saved</span>
              {/if}
              <span class="spacer"></span>
              <button
                class="chip"
                class:on={!!asText}
                aria-pressed={!!asText}
                disabled={!!textWhy}
                title={textWhy ?? (asText ? "Back to picking sides; text you typed is dropped" : "Type the whole file, with git’s markers around the conflicts still open")}
                onclick={() => (asText ? stopText() : startText())}
              >
                Edit as Text
              </button>
              {#if !asText}
                <button class="button small" onclick={() => takeFile(leftSide)} title="Use {leftLabel}’s whole file">All {leftLabel}</button>
                <button class="button small" onclick={() => takeFile(rightSide)} title="Use {rightLabel}’s whole file">All {rightLabel}</button>
              {/if}
              <button
                class="button primary small"
                disabled={open_ > 0}
                title={open_ ? (asText ? "Remove every conflict marker first" : "Decide every conflict first") : undefined}
                onclick={markResolved}
              >
                Mark Resolved
              </button>
            </header>
            {#if oldDraft && comparing}
              <div class="olddraft">
                <span class="ellipsis">Your draft from {oldWhen}, for other versions of {p.name}. It isn’t put back by itself.</span>
                <span class="spacer"></span>
                <button class="button small" disabled={!!useWhy} title={useWhy ?? "Edit this text as the result of the file"} onclick={useOld}>Use as Text</button>
                <button class="button small" onclick={dropOld}>Discard Draft</button>
                <button class="button small" onclick={() => (comparing = false)}>Back to Result</button>
              </div>
            {:else if oldDraft}
              <div class="olddraft">
                <span class="ellipsis">A draft from {oldWhen} was made for other versions of {p.name}, so it isn’t put back.</span>
                <span class="spacer"></span>
                <button class="button small" onclick={() => (comparing = true)}>Compare</button>
                <button class="button small" onclick={dropOld}>Discard Draft</button>
              </div>
            {/if}
            {#if leaving}
              <div class="leaving">
                <span>Go back to picking sides? The text you typed in {p.name} is dropped.</span>
                <span class="spacer"></span>
                <button class="button small" onclick={() => (leaving = false)}>Keep Editing</button>
                <button class="button small" onclick={dropText}>Drop Text</button>
              </div>
            {/if}
            <div class="code mono selectable">
              {#if oldDraft && comparing}
                {#each oldLines as line, i (i)}
                  <div class="line"><span class="no">{i + 1}</span><span class="text">{line}</span></div>
                {:else}
                  <p class="placeholder">An empty file: every line was taken out.</p>
                {/each}
              {:else if asText}
                <EditBox value={asText.text} tintOf={textTint} label="The whole result of {p.name}" onedit={typedText} bind:rows={textRows} />
              {:else}
                {#each resultItems as item, i (itemKey(item, i))}
                  {#if item.kind === "open"}
                    <div class="open" class:current={at === item.k} data-conflict={item.k}>
                      <div class="open-head">
                        <span class="ellipsis">Conflict {item.k + 1} · both sides changed these lines</span>
                        <span class="spacer"></span>
                        <button class="pick" style:color={plate(leftColor)} onclick={() => decide(item.k, "left")}>{leftLabel}</button>
                        <button class="pick" style:color={plate(rightColor)} onclick={() => decide(item.k, "right")}>{rightLabel}</button>
                        <button class="pick" onclick={() => decide(item.k, "both")}>Both</button>
                        {#if !textWhy}<button class="pick" title="Type the lines yourself, starting from both sides" onclick={() => edit(item.k)}>Edit</button>{/if}
                      </div>
                      <div class="placeholder">Pick a side, keep both, or edit</div>
                    </div>
                  {:else if item.kind === "bar"}
                    <div class="bar" class:current={at === item.k} data-conflict={item.k}>
                      <span>
                        Conflict {item.k + 1}: {item.choice === "both" ? `both, ${leftLabel} first` : `took ${item.choice === "left" ? leftLabel : rightLabel}`}
                      </span>
                      {#if !textWhy}<button class="undo" onclick={() => edit(item.k)}>Edit</button>{/if}
                      <button class="undo" onclick={() => decide(item.k, null)}>Undo</button>
                    </div>
                  {:else if item.kind === "typed"}
                    <div class="bar" class:current={at === item.k} data-conflict={item.k}>
                      <span>Conflict {item.k + 1}: typed by you{item.value === "" ? " · no lines, the conflict goes" : ""}</span>
                      <button class="undo" onclick={() => decide(item.k, null)}>Undo</button>
                    </div>
                    <div class="typed">
                      <EditBox
                        value={item.value}
                        first={item.no}
                        tintOf={() => "color-mix(in srgb, var(--accent) 9%, transparent)"}
                        label="Conflict {item.k + 1} of {p.name}"
                        focus={fresh === item.k}
                        onedit={(value) => decide(item.k, { lines: splitLines(value) })}
                      />
                    </div>
                  {:else}
                    <div
                      class="line"
                      class:picked={item.from !== null}
                      style:background={item.from ? tint(colorFor(item.from), "bar") : undefined}
                      style:border-radius={radius(item.edge)}
                    >
                      <span class="no">{item.no}</span><span class="text">{item.text}</span>
                    </div>
                  {/if}
                {/each}
              {/if}
            </div>
          </section>
        {/key}
      {/if}
    {/if}
  </div>
</div>

<style>
  /* No cards: the files on the graph's paper, the two sides and the result parted by hairlines. */
  .conflicts {
    flex-grow: 1;
    display: flex;
    min-height: 0;
    border-top: 1px solid var(--sep);
  }
  .files {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: var(--space-2) var(--space-2) var(--space-3);
    border-right: 1px solid var(--sep);
    background: var(--graph-bg);
  }
  .ghead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 6px 8px 6px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .sub-head {
    margin-top: 12px;
  }
  .count {
    font-weight: 400;
  }
  .track {
    margin: 0 8px 8px;
    height: 4px;
    border-radius: var(--radius-pill);
    background: var(--field);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 4px;
    background: var(--green);
    transition: width 0.25s;
  }
  .list {
    flex-grow: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .file {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 7px 8px;
    border-radius: var(--radius-m);
    text-align: left;
    width: 100%;
  }
  .file.on {
    background: var(--side-sel);
  }
  .mark {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    margin-top: 1px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .mark.warn {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .mark.ok {
    background: var(--green-soft);
    color: var(--green);
  }
  .fwords {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .dir {
    color: var(--text2);
  }
  .sub {
    font-size: var(--text-caption);
  }
  .orange {
    color: var(--orange);
  }
  .green {
    color: var(--green);
  }
  .small {
    font-size: var(--text-small);
    white-space: nowrap;
  }
  .auto {
    display: flex;
    gap: 8px;
    padding: 4px 8px 4px 12px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .check {
    color: var(--green);
  }
  .tip {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
  }
  .tip:empty {
    display: none;
  }
  .hint {
    margin: 8px 0 0;
    padding: 10px 12px;
    border-radius: var(--radius-l);
    background: var(--field);
    font-size: var(--text-caption);
    line-height: 1.45;
    color: var(--text2);
  }
  .work {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .sides {
    display: flex;
    height: 42%;
    min-height: 150px;
    border-bottom: 1px solid var(--sep);
  }
  .sides > .pane + .pane {
    border-left: 1px solid var(--sep);
  }
  .pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--win);
    overflow: hidden;
  }
  .result {
    flex: 1;
  }
  .pane-head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    flex-shrink: 0;
    padding: 0 10px;
    border-bottom: 1px solid var(--sep);
    font-size: var(--text-small);
  }
  .cap {
    padding: 1px 8px;
    border-radius: var(--radius-m);
    font-weight: 500;
    white-space: nowrap;
  }
  .note {
    color: var(--text2);
    min-width: 0;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex-grow: 1;
  }
  .code {
    flex-grow: 1;
    overflow: auto;
    padding: 6px 8px 10px;
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    tab-size: var(--tab);
    color: var(--code);
  }
  .line {
    display: grid;
    grid-template-columns: 36px 1fr;
  }
  /* Side panes are narrow: lines scroll sideways instead of wrapping. */

  .no {
    color: var(--text2);
    opacity: 0.7;
    text-align: right;
    padding-right: 10px;
    user-select: none;
    -webkit-user-select: none;
  }
  .text {
    white-space: pre-wrap;
    word-break: break-all;
    padding-right: 8px;
  }
  .gone {
    font-style: italic;
    color: var(--text2);
    font-family: var(--font);
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    margin-top: 4px;
    padding: 0 4px 0 10px;
    border-radius: var(--radius-m) var(--radius-m) 0 0;
    font-family: var(--font);
    font-size: var(--text-caption);
    font-weight: 600;
  }
  .take,
  .pick {
    height: 20px;
    padding: 0 9px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-family: var(--font);
    font-size: var(--text-caption);
    font-weight: 600;
  }
  .in {
    font-weight: 500;
    padding-right: 6px;
  }
  .inblock:last-child,
  .inblock:has(+ :not(.inblock)) {
    border-radius: 0 0 var(--radius-m) var(--radius-m);
    margin-bottom: 4px;
  }
  .fold {
    margin: 4px 0;
    padding: 0 12px;
    height: 22px;
    line-height: 22px;
    border-radius: var(--radius-m);
    background: var(--field);
    color: var(--text2);
    font-family: var(--font);
    font-size: var(--text-caption);
  }
  .open {
    margin: 4px 0;
    border-radius: var(--radius-m);
    background: var(--orange-soft);
  }
  .open-head {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 6px 0 10px;
    font-family: var(--font);
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--orange);
  }
  .placeholder {
    padding: 0 10px 6px 46px;
    font-style: italic;
    color: var(--text2);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    padding: 0 6px 2px 46px;
    font-family: var(--font);
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .undo {
    font-size: var(--text-caption);
    color: var(--accent-text);
  }
  .filebar {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    flex-shrink: 0;
    padding: 0 10px 0 14px;
    border-bottom: 1px solid var(--sep);
    background: var(--win);
    font-size: var(--text-small);
  }
  .fname {
    font-weight: 600;
    flex-shrink: 0;
    max-width: 45%;
  }
  .fname .dir {
    font-weight: 400;
  }
  .kind {
    color: var(--text2);
    min-width: 0;
    flex-shrink: 2;
  }
  .waitnote {
    color: var(--text2);
    white-space: nowrap;
  }
  /* A toggle that reads as one: lit while on. */
  .chip {
    height: 24px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    white-space: nowrap;
    color: var(--text2);
    border: 0.5px solid var(--sep);
  }
  .chip.on {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-color: transparent;
    color: var(--accent-text);
    font-weight: 500;
  }
  .chip:disabled {
    opacity: 0.45;
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 4px;
  }
  .stepper .note {
    margin-left: 4px;
    white-space: nowrap;
  }
  .step {
    width: 22px;
    height: 22px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--icon);
  }
  .step .icon {
    width: 13px;
    height: 13px;
  }
  .step:disabled {
    opacity: 0.4;
  }
  .leaving {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    padding: 6px 10px 6px 14px;
    border-bottom: 1px solid var(--sep);
    background: var(--orange-soft);
    font-size: var(--text-small);
  }
  .typed {
    margin: 0 0 4px;
  }
  /* A draft that no longer fits the file: a quiet note, not a warning. */
  .olddraft {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    padding: 6px 10px 6px 14px;
    border-bottom: 1px solid var(--sep);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    font-size: var(--text-small);
  }
  .saved {
    color: var(--text2);
  }
  .muted {
    color: var(--text2);
  }
  /* The conflict stepped to: a quiet ring in the accent. */
  .open.current,
  .chead.current {
    box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .bar.current span:first-child {
    color: var(--text);
    font-weight: 600;
  }
  .empty {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text2);
    text-align: center;
    padding: 24px;
  }
  .empty .title {
    font-size: var(--text-heading);
    font-weight: 700;
    color: var(--text);
  }
  .empty .big {
    font-size: var(--text-display);
    color: var(--green);
  }
  .explain {
    max-width: 420px;
    line-height: 1.5;
  }
  .error {
    color: var(--red);
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

</style>
