<script lang="ts">
  // Browse Files at This Commit: the repository as one commit has it, its folders on the left,
  // read one at a time, and the picked file on the right as text or a picture. A file is saved
  // elsewhere as it is there, followed back through File History from that commit, compared
  // with the file on disk, or taken onto disk as it is there.

  import { untrack } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api } from "./api";
  import { nav } from "./nav.svelte";
  import { confirm } from "./confirm.svelte";
  import type { FileAt, HistoryRow, TreeItem } from "./types";
  import { shortId, splitPath } from "./format";
  import { languageOf, paint } from "./syntax";
  import { bytes, imageType } from "./unusual";
  import Lazy, { BLOCK } from "./Lazy.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { findFiles, foldersOf, leftOf, linesOf, stepRow, treeRows } from "./browseTree";
  import { NEWEST_SAVED } from "./localHistory";
  import { takeVersionRequest, type CommitFile } from "./commitFiles";
  import WorkingCopyPanel from "./WorkingCopyPanel.svelte";

  let {
    rev,
    label = null,
    path = null,
    lookup,
    copy,
    onPath,
    version = 0,
  }: {
    /** The commit whose files these are. */
    rev: string;
    /** The branch or tag it was opened from. */
    label?: string | null;
    /** The file shown. */
    path?: string | null;
    lookup: (id: string) => HistoryRow | undefined;
    copy: (text: string, done: string) => void;
    /** Another file was picked: Back comes back to it. */
    onPath: (path: string | null) => void;
    /** Goes up on every reload, so Compare with Working Copy follows the file on disk. */
    version?: number;
  } = $props();

  const row = $derived(lookup(rev));
  /** Each folder's entries once read, by its path ("" for the top). */
  let folders = $state.raw(new Map<string, TreeItem[]>());
  let open = $state.raw(new Set<string>());
  let error = $state<string | null>(null);
  let query = $state("");
  /** Every file's path, read once a file is looked for by name. */
  let all = $state.raw<string[] | null>(null);
  let file = $state.raw<FileAt | null>(null);
  let fileError = $state<string | null>(null);
  let picture = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; label: string; entries: MenuEntry[] } | null>(null);
  let list = $state<HTMLDivElement>();
  /** The row the arrows are on: a folder or the file shown. */
  let cursor = $state<string | null>(null);
  /** Compare with Working Copy, of the file shown, in place of its text. */
  let comparing = $state(false);
  // Another file or commit shows its text again.
  $effect(() => {
    rev;
    path;
    untrack(() => (comparing = false));
  });
  /** A file of the commit, as Take This Version and the comparison know it. */
  const asFile = (target: string): CommitFile => ({ path: target, status: "modified" });

  // Another commit starts afresh.
  $effect(() => {
    const at = rev;
    untrack(() => {
      folders = new Map();
      open = new Set();
      all = null;
      error = null;
      readFolder(at, "");
    });
  });

  // The file asked for, read whenever it or the commit changes, with its folders open in the
  // tree: it may come from a menu elsewhere, or from Back.
  $effect(() => {
    const at = rev;
    const wanted = path;
    untrack(() => {
      readFile(at, wanted);
      if (wanted) reveal(wanted);
    });
  });

  async function readFolder(at: string, dir: string) {
    if (folders.has(dir)) return;
    try {
      const items = await api.treeAt(at, dir);
      if (rev !== at) return;
      folders = new Map(folders).set(dir, items);
    } catch (err) {
      if (rev === at && !dir) error = String(err);
      else if (rev === at) confirm.say(String(err), "error");
    }
  }

  async function readFile(at: string, wanted: string | null) {
    if (picture) URL.revokeObjectURL(picture);
    picture = null;
    fileError = null;
    if (!wanted) {
      file = null;
      return;
    }
    try {
      const read = await api.fileAt(at, wanted);
      if (rev !== at || path !== wanted) return;
      file = read;
      const type = imageType(wanted);
      if (type && (read.binary || type === "image/svg+xml")) {
        const data = await api.sourceBytes({ kind: "blob", id: read.id });
        if (rev === at && path === wanted) picture = URL.createObjectURL(new Blob([data as BlobPart], { type }));
      }
    } catch (err) {
      if (rev === at && path === wanted) {
        file = null;
        fileError = String(err);
      }
    }
  }

  $effect(() => () => {
    if (picture) URL.revokeObjectURL(picture);
  });

  /** Open the folders a file is in, and show it. */
  async function reveal(target: string) {
    const at = rev;
    const up = foldersOf(target);
    open = new Set([...open, ...up]);
    for (const dir of up) await readFolder(at, dir);
    cursor = target;
    requestAnimationFrame(() => list?.querySelector(`[data-path="${CSS.escape(target)}"]`)?.scrollIntoView({ block: "nearest" }));
  }

  function toggle(dir: string, on = !open.has(dir)) {
    const next = new Set(open);
    if (on) next.add(dir);
    else next.delete(dir);
    open = next;
    if (on) readFolder(rev, dir);
  }

  function pick(item: TreeItem) {
    cursor = item.path;
    if (item.kind === "folder") toggle(item.path);
    else if (item.kind !== "submodule") onPath(item.path);
  }

  const rows = $derived(treeRows(folders, open));
  const found = $derived(query.trim() && all ? findFiles(all, query) : null);
  /** The rows the arrows go through: the files found, or the tree. */
  const order = $derived(found ?? rows.map((r) => r.item.path));

  // Looking for a file by name reads the commit's whole list once.
  $effect(() => {
    if (!query.trim() || all) return;
    const at = rev;
    untrack(() =>
      api.filesAt(at).then(
        (paths) => rev === at && (all = paths),
        (err) => confirm.say(String(err), "error"),
      ),
    );
  });

  function keydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = stepRow(order, cursor, e.key === "ArrowDown" ? 1 : -1);
      if (!next) return;
      cursor = next;
      const item = rows.find((r) => r.item.path === next)?.item;
      if (found || (item && item.kind !== "folder" && item.kind !== "submodule")) onPath(next);
      requestAnimationFrame(() => list?.querySelector(`[data-path="${CSS.escape(next)}"]`)?.scrollIntoView({ block: "nearest" }));
    } else if (!found && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
      const at = rows.find((r) => r.item.path === cursor);
      if (!at) return;
      e.preventDefault();
      if (e.key === "ArrowRight") {
        if (at.item.kind === "folder") toggle(at.item.path, true);
        return;
      }
      const step = leftOf(at, open);
      if (step && "close" in step) toggle(step.close, false);
      else if (step) cursor = step.go;
    } else if (e.key === "Enter" && cursor) {
      const item = rows.find((r) => r.item.path === cursor)?.item;
      if (item) pick(item);
    }
  }

  async function saveAs(target: string) {
    const name = splitPath(target).name;
    const to = await save({ title: `Save ${name} as it is in ${shortId(rev)}`, defaultPath: name });
    if (!to) return;
    try {
      await api.saveFileAt(rev, target, to);
      confirm.say(`Saved ${name} of ${shortId(rev)}.`, "done");
    } catch (err) {
      confirm.say(String(err), "error");
    }
  }

  function compare(target: string) {
    if (target !== path) onPath(target);
    // After the effect that closes it for another file.
    requestAnimationFrame(() => (comparing = true));
  }

  // Blame starts at this commit: the file as it is here, and the commits that made it so.
  const fileHistory = (target: string) => nav.openFile(target, rev, rev);

  function rowMenu(e: MouseEvent, item: TreeItem) {
    e.preventDefault();
    const entries: MenuEntry[] = [{ kind: "header", label: item.path }];
    if (item.kind !== "folder" && item.kind !== "submodule") {
      entries.push(
        { kind: "item", label: "File History", icon: menuIcons.history, run: () => fileHistory(item.path) },
        { kind: "item", label: "Local History", icon: menuIcons.history, run: () => nav.openFile(item.path, NEWEST_SAVED) },
        { kind: "item", label: "Save As…", icon: menuIcons.patch, run: () => saveAs(item.path) },
        { kind: "sep" },
        { kind: "item", label: "Take This Version…", icon: menuIcons.checkout, run: () => confirm.runner?.(takeVersionRequest(asFile(item.path), rev, row?.summary ?? null)) },
        { kind: "item", label: "Compare with Working Copy", icon: menuIcons.compare, run: () => compare(item.path) },
        { kind: "sep" },
      );
    }
    entries.push({ kind: "item", label: "Copy Path", icon: menuIcons.copy, run: () => copy(item.path, `Copied ${item.path}.`) });
    menu = { x: e.clientX, y: e.clientY, label: `Menu of ${item.name}`, entries };
  }

  const language = $derived(path ? languageOf(path) : null);
  const lines = $derived(file?.text != null ? linesOf(file.text) : []);
  const digits = $derived(String(lines.length).length);
  const blocks = $derived(Array.from({ length: Math.ceil(lines.length / BLOCK) }, (_, i) => i * BLOCK));
  const kindIcon: Record<TreeItem["kind"], string> = {
    folder: "M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z",
    file: "M4.5 2.5h5l3 3v8h-8zM9.5 2.5v3h3",
    executable: "M4.5 2.5h5l3 3v8h-8zM9.5 2.5v3h3M6.5 9l1.5 1.5L6.5 12",
    link: "M7 9.5a2.5 2.5 0 0 0 3.5 0l2-2a2.5 2.5 0 0 0-3.5-3.5l-.75.75M9 6.5a2.5 2.5 0 0 0-3.5 0l-2 2A2.5 2.5 0 0 0 7 12l.75-.75",
    submodule: "M3 3.5h10v9H3zM3 6.5h10",
  };
</script>

{#snippet entry(item: TreeItem, depth: number, flat: boolean)}
  <button
    class="entry"
    class:on={item.path === path}
    class:cursor={item.path === cursor}
    class:dim={item.kind === "submodule"}
    data-path={item.path}
    style:padding-left="{8 + depth * 16}px"
    tabindex="-1"
    onclick={() => pick(item)}
    oncontextmenu={(e) => rowMenu(e, item)}
    title={item.kind === "submodule" ? `${item.path}: a submodule, another repository's commit` : item.path}
  >
    {#if item.kind === "folder"}
      <svg class="chevron" class:open={open.has(item.path)} viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
    {:else if !flat}
      <span class="chevron"></span>
    {/if}
    <svg class="icon" class:folder={item.kind === "folder"} viewBox="0 0 16 16"><path d={kindIcon[item.kind]} /></svg>
    {#if flat}
      {@const where = splitPath(item.path)}
      <span class="name"><span class="dir">{where.dir}</span>{where.name}</span>
    {:else}
      <span class="name">{item.name}</span>
    {/if}
    {#if item.size}<span class="size">{bytes(item.size)}</span>{/if}
  </button>
{/snippet}

<div class="screen">
  <section class="side" aria-label="Files at {shortId(rev)}">
    <div class="top">
      <span class="sha mono">{shortId(rev)}</span>
      {#if label}<span class="label">{label}</span>{/if}
      <span class="summary" title={row?.summary}>{row?.summary ?? ""}</span>
    </div>
    <div class="find">
      <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5 14 14" /></svg>
      <input bind:value={query} placeholder="Find a file" aria-label="Find a file" spellcheck="false" onkeydown={keydown} />
      {#if query}<button class="clear" aria-label="Clear" onclick={() => (query = "")}>×</button>{/if}
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="list" bind:this={list} role="tree" aria-label="Folders and files" tabindex="0" onkeydown={keydown}>
      {#if error}
        <p class="error" role="alert">{error}</p>
      {:else if query.trim()}
        {#if !found}
          <p class="hint">Listing the files…</p>
        {:else if !found.length}
          <p class="hint">No file of {shortId(rev)} has “{query.trim()}” in its path.</p>
        {:else}
          {#each found as hit (hit)}
            {@render entry({ name: splitPath(hit).name, path: hit, kind: "file", size: 0 }, 0, true)}
          {/each}
        {/if}
      {:else if !folders.has("")}
        <p class="hint">Reading the files…</p>
      {:else if !rows.length}
        <p class="hint">This commit has no files.</p>
      {:else}
        {#each rows as r (r.item.path)}
          {@render entry(r.item, r.depth, false)}
        {/each}
      {/if}
    </div>
  </section>

  <section class="main" aria-label={comparing ? "Compare with Working Copy" : (path ?? "File")}>
    {#if path && comparing}
      <WorkingCopyPanel commit={rev} summary={row?.summary ?? null} file={asFile(path)} color={row?.graph.color ?? 0} {version} onClose={() => (comparing = false)} />
    {:else if path}
      {@const where = splitPath(path)}
      <div class="head">
        <span class="path" title={path}><span class="dir">{where.dir}</span><b>{where.name}</b></span>
        {#if file}<span class="muted">{bytes(file.size)}{file.text != null ? ` · ${lines.length.toLocaleString()} ${lines.length === 1 ? "line" : "lines"}` : ""}</span>{/if}
        <span class="spacer"></span>
        <button class="button" onclick={() => fileHistory(path)} title="The commits that changed it, from {shortId(rev)} back">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3l2 1.5" /></svg>File History
        </button>
        <button class="button" onclick={() => saveAs(path)} title="Write it somewhere as it is in {shortId(rev)}">Save As…</button>
      </div>
      {#if fileError}
        <p class="error" role="alert">{fileError}</p>
      {:else if !file}
        <p class="hint pad">Reading {where.name}…</p>
      {:else}
        {#if file.lfs}<p class="note">Kept in Git LFS: this is its pointer. The file itself comes with Git LFS’s download.</p>{/if}
        {#if picture}
          <div class="picture"><img src={picture} alt={where.name} /></div>
        {:else if file.text == null}
          <div class="card">
            <b>{file.binary ? "A binary file" : "Too big to show here"}</b>
            <span>{bytes(file.size)}. Save As writes it where another app can open it.</span>
            <button class="button" onclick={() => saveAs(path)}>Save As…</button>
          </div>
        {:else}
          <div class="code selectable" style:--digits={digits}>
            {#each blocks as start (start)}
              {@const part = lines.slice(start, start + BLOCK)}
              <Lazy lines={part.length} of={file} name={`${start}`}>
                {#each part as text, j (j)}
                  <div class="row"><span class="no">{start + j + 1}</span><span class="text">{#each paint(text || " ", null, language) as piece, k (k)}<span class={piece.role ? `syn-${piece.role}` : undefined}>{piece.text}</span>{/each}</span></div>
                {/each}
              </Lazy>
            {/each}
          </div>
        {/if}
      {/if}
    {:else}
      <p class="hint pad">Pick a file on the left to read it as it is in {shortId(rev)}.</p>
    {/if}
  </section>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .screen {
    flex-grow: 1;
    display: flex;
    min-height: 0;
    border-top: 1px solid var(--sep);
  }
  .side {
    width: 320px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--sep);
    background: var(--graph-bg);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px 6px;
    font-size: var(--text-small);
    min-width: 0;
  }
  .sha {
    font-weight: 600;
  }
  .label {
    padding: 1px 7px;
    border-radius: var(--radius-pill);
    background: var(--field);
    font-size: var(--text-caption);
    font-weight: 600;
    white-space: nowrap;
  }
  .summary {
    min-width: 0;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .find {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 4px 8px 6px;
    padding: 0 8px;
    height: 28px;
    border-radius: var(--radius-m);
    background: var(--field);
  }
  .find input {
    flex-grow: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--text-small);
    outline: none;
  }
  .find .icon {
    width: 13px;
    height: 13px;
    color: var(--text2);
  }
  .clear {
    color: var(--text2);
    font-size: var(--text-heading);
  }
  .list {
    flex-grow: 1;
    overflow-y: auto;
    padding: 0 8px 8px;
    outline: none;
  }
  .entry {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding-right: 8px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
    text-align: left;
    color: var(--text);
  }
  .entry:hover {
    background: var(--field);
  }
  .entry.on {
    background: var(--side-sel);
    font-weight: 600;
  }
  .list:focus-visible .entry.cursor {
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  .entry.dim {
    color: var(--text2);
  }
  .chevron {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    color: var(--text2);
    transition: transform 0.12s;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .entry .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    color: var(--icon);
  }
  .entry .icon.folder {
    color: var(--accent);
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dir,
  .size,
  .muted {
    color: var(--text2);
  }
  .size {
    font-size: var(--text-caption);
    font-weight: 400;
    flex-shrink: 0;
  }
  .main {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--sep);
    flex-shrink: 0;
    min-width: 0;
  }
  .path {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .muted {
    font-size: var(--text-small);
    white-space: nowrap;
  }
  .spacer {
    flex-grow: 1;
  }
  .button .icon {
    width: 13px;
    height: 13px;
  }
  .code {
    flex-grow: 1;
    overflow: auto;
    min-height: 0;
    margin: 12px;
    padding: 6px 0;
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    tab-size: var(--tab);
    color: var(--code-fg, var(--code));
    background: var(--code-bg, transparent);
    border-radius: var(--radius-l);
  }
  .row {
    display: grid;
    grid-template-columns: max(44px, calc(var(--digits, 4) * 1ch + 20px)) 1fr;
    min-width: max-content;
  }
  .no {
    color: var(--code-ln, color-mix(in srgb, var(--code-dim, var(--text2)) 70%, transparent));
    text-align: right;
    padding-right: 12px;
    user-select: none;
    -webkit-user-select: none;
  }
  .text {
    white-space: pre;
    padding-right: 16px;
  }
  .picture {
    flex-grow: 1;
    overflow: auto;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: repeating-conic-gradient(var(--field) 0 25%, transparent 0 50%) 0 0 / 16px 16px;
  }
  .picture img {
    max-width: 100%;
    max-height: 100%;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    margin: 24px;
    padding: 16px;
    border-radius: var(--radius-l);
    background: var(--field);
    font-size: var(--text-small);
  }
  .note {
    margin: 10px 16px 0;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .hint {
    margin: 12px 8px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .hint.pad {
    margin: 20px;
  }
  .error {
    margin: 20px;
    color: var(--red);
  }
</style>
