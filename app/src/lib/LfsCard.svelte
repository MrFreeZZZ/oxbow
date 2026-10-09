<script lang="ts">
  // In place of a diff: a file too big for git, with the way into Git LFS, or a file Git LFS
  // keeps, with the pointer git actually stores for it.

  import { api } from "./api";
  import { confirm } from "./confirm.svelte";
  import { mac } from "./keys";
  import { nav } from "./nav.svelte";
  import { installRequest, lfs, lfsTrackRequest, untrackRequest } from "./lfs.svelte";
  import TermBlock from "./TermBlock.svelte";
  import type { FileDiff, LfsPointer } from "./types";
  import { bytes, bytesDelta, imageType } from "./unusual";

  let {
    diff,
    kind,
    working,
  }: {
    diff: FileDiff;
    kind: "big" | "lfs";
    /** The file is an uncommitted change, so it can be tracked, untracked and previewed. */
    working: boolean;
  } = $props();

  const repoName = $derived(nav.repo?.split(/[\\/]/).filter(Boolean).pop() ?? "");

  const file = $derived(diff.file);
  const name = $derived(file.path.slice(file.path.lastIndexOf("/") + 1));
  const dir = $derived(file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : "");
  const size = $derived(file.newSize ?? file.oldSize ?? 0);
  const pattern = $derived(lfs.patternOf(file.path));
  const pointer = $derived<LfsPointer | null>(file.lfs?.new ?? file.lfs?.old ?? null);
  const others = $derived(lfs.patterns.map((p) => p.pattern).filter((p) => p !== pattern));
  const canPreview = $derived(working && file.status !== "deleted");

  const VIDEO = /\.(mov|mp4|m4v|avi|mkv|webm)$/i;
  const AUDIO = /\.(wav|mp3|aiff?|flac|m4a|ogg)$/i;
  const ARCHIVE = /\.(zip|tar|gz|tgz|7z|rar|dmg|iso)$/i;
  const kindWord = $derived(
    VIDEO.test(name) ? "Video" : AUDIO.test(name) ? "Audio" : imageType(name) ? "Picture" : ARCHIVE.test(name) ? "Archive" : file.binary ? "Binary file" : "Text file",
  );

  function run(request: ReturnType<typeof lfsTrackRequest> | null) {
    if (request) confirm.runner?.(request);
  }

  function track() {
    if (lfs.ready) run(lfsTrackRequest(file.path, size));
    else if (lfs.status?.version || lfs.status?.brew) run(installRequest(repoName));
    else api.openLfsDownload().catch((err) => confirm.say(String(err)));
  }

  function preview() {
    api.quickLook(file.path).catch((err) => confirm.say(String(err)));
  }

  const pointerLines = (p: LfsPointer) => [
    { kind: "out" as const, text: "version https://git-lfs.github.com/spec/v1" },
    { kind: "out" as const, text: `oid ${p.oid}` },
    { kind: "out" as const, text: `size ${p.size}` },
  ];
  const pointerText = (p: LfsPointer) => pointerLines(p).map((l) => l.text);
</script>

<div class="card">
  {#if kind === "big"}
    <div class="top">
      <span class="badge warn"><svg class="icon" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M6.5 6v4l3.5-2z" /></svg></span>
      <div>
        <h3>{name} is {bytes(size)}</h3>
        <p class="sub">{[dir, kindWord].filter(Boolean).join(" · ")}</p>
      </div>
    </div>
    <p>
      {size >= 100 * 1024 * 1024 ? "GitHub refuses files over 100 MB" : "GitHub warns about files over 50 MB"}, and a file committed to git stays in every clone forever,
      even after you delete it. Git LFS keeps it on the LFS server and commits a small pointer instead.{#if others.length}
        {" "}{repoName || "This repository"} already uses Git LFS for {others.slice(0, 3).join(", ")}{others.length > 3 ? " and more" : ""}.{/if}
    </p>
    {#if working}
      <div class="actions">
        <button class="button primary" onclick={track}>{lfs.ready ? "Track with Git LFS…" : lfs.status?.version ? "Turn On Git LFS…" : lfs.status?.brew ? "Install Git LFS…" : "Download Git LFS…"}</button>
        {#if canPreview}<button class="button" onclick={preview}>{mac ? "Quick Look" : "Open"}</button>{/if}
      </div>
    {/if}
  {:else}
    <div class="top">
      <span class="badge"><svg class="icon" viewBox="0 0 16 16"><path d="M3 4a5 1.8 0 1 0 10 0a5 1.8 0 1 0-10 0M3 4v8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8V4M3 8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8" /></svg></span>
      <div>
        <h3>{working ? `${name} goes to Git LFS` : `${name} is kept in Git LFS`}</h3>
        <p class="sub">
          {#if working}
            {pattern ? `Tracked by ${pattern} in .gitattributes` : "Tracked by .gitattributes"} · uploads when you push
          {:else}
            {file.status === "deleted" ? "Deleted" : bytes(size)}{file.lfs?.old && file.lfs?.new && file.oldSize !== file.newSize ? ` · ${bytesDelta((file.newSize ?? 0) - (file.oldSize ?? 0))}` : ""}
          {/if}
        </p>
      </div>
    </div>
    {#if file.lfs?.old && file.lfs?.new && file.lfs.old.oid !== file.lfs.new.oid}
      <dl class="facts">
        <dt>Size</dt>
        <dd class="mono">{bytes(file.lfs.old.size)} <span class="arrow">→</span> <b>{bytes(file.lfs.new.size)}</b></dd>
        <dt>Object</dt>
        <dd class="mono">{file.lfs.old.oid.replace("sha256:", "").slice(0, 7)} <span class="arrow">→</span> <b>{file.lfs.new.oid.replace("sha256:", "").slice(0, 7)}</b></dd>
      </dl>
    {/if}
    {#if pointer}
      <TermBlock where="{name} · pointer" lines={pointerLines(pointer)} copy={pointerText(pointer)} />
      <p>
        {#if working}
          The commit stores only this pointer. The file uploads to the LFS server when you push; teammates get it when they pull.
        {:else}
          Git stores only this pointer; the file itself is on the LFS server.
        {/if}
      </p>
    {:else if working}
      <p>Once staged, git stores a three-line pointer in its place, and the file uploads to the LFS server when you push.</p>
    {/if}
    {#if working}
      <div class="actions">
        {#if canPreview}<button class="button" onclick={preview}>{mac ? "Quick Look" : "Open"}</button>{/if}
        {#if pattern && lfs.ready}<button class="button" onclick={() => run(untrackRequest(pattern!))}>Stop Tracking…</button>{/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 8px 6px 4px;
  }
  .top {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .badge {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    background: var(--field);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--text);
  }
  .badge.warn {
    color: var(--orange);
    background: var(--orange-soft);
  }
  .badge .icon {
    width: 18px;
    height: 18px;
  }
  h3 {
    margin: 2px 0 4px;
    font-size: 13px;
    font-weight: 600;
  }
  p {
    margin: 0;
    color: var(--text2);
    line-height: 1.4;
  }
  .sub {
    font-size: 12px;
  }
  .facts {
    display: grid;
    grid-template-columns: 100px 1fr;
    row-gap: 6px;
    margin: 0;
    padding: 9px 12px;
    border-radius: 10px;
    background: var(--field);
    font-size: 12px;
  }
  dt {
    color: var(--text2);
  }
  dd {
    margin: 0;
  }
  .arrow {
    color: var(--text2);
    margin: 0 6px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .button {
    height: 28px;
    min-width: 120px;
    flex-shrink: 0;
    white-space: nowrap;
    padding: 0 14px;
    border-radius: 14px;
    border: 1px solid var(--sep);
    font-size: 12px;
  }
  .button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
    font-weight: 600;
  }
</style>
