<script lang="ts">
  // Under the toolbar while the repository keeps files in Git LFS but git-lfs can't fetch them
  // here: those files are pointer text until it is installed and turned on.

  import { api } from "./api";
  import { confirm } from "./confirm.svelte";
  import { installRequest, lfs } from "./lfs.svelte";

  let { repoName, run }: { repoName: string; run: (request: NonNullable<ReturnType<typeof installRequest>>) => void } = $props();

  const installed = $derived(!!lfs.status?.version);
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

  function act() {
    const request = installRequest(repoName);
    if (request) run(request);
    else api.openLfsDownload().catch((err) => confirm.say(String(err)));
  }
</script>

<section class="banner" aria-label="Git LFS">
  <span class="tile">
    <svg class="icon" viewBox="0 0 16 16"><path d="M3 4a5 1.8 0 1 0 10 0a5 1.8 0 1 0-10 0M3 4v8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8V4M3 8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8" /></svg>
  </span>
  <span class="words">
    <span class="title">{installed ? "Git LFS isn’t turned on" : "Git LFS isn’t installed"}</span>
    <span class="sub">{plural(lfs.files, "file")} in {repoName} {lfs.files === 1 ? "is" : "are"} stored with Git LFS, so here {lfs.files === 1 ? "it is" : "they are"} only pointer text files.</span>
  </span>
  <button class="btn" onclick={() => (lfs.dismissed = true)}>Not Now</button>
  <button class="btn go" onclick={act}>{installed ? "Turn On Git LFS…" : lfs.status?.brew ? "Install Git LFS…" : "Download Git LFS…"}</button>
</section>

<style>
  .banner {
    margin: 0 12px 10px 12px;
    padding: 0 12px 0 14px;
    height: 58px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    border-radius: 16px;
    border: 0.5px solid color-mix(in srgb, var(--orange) 30%, transparent);
    background: color-mix(in srgb, var(--orange-soft) 55%, transparent);
  }
  .tile {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--orange);
    background: var(--orange-soft);
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 12px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btn {
    flex-shrink: 0;
    min-width: 120px;
    height: 30px;
    padding: 0 14px;
    border-radius: 15px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
</style>
