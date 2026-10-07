<script lang="ts">
  // Clone Repository: a URL checked as it is typed, the folder it goes to, submodules and shallow
  // switches, the exact command, then git's three phases as it runs.

  import { listen } from "@tauri-apps/api/event";
  import { open as chooseFolder } from "@tauri-apps/plugin-dialog";
  import { api } from "./api";
  import { plate, tint } from "./format";
  import { prefs } from "./prefs.svelte";
  import { join, nameFromUrl, start } from "./start.svelte";
  import TermBlock, { type TermLine } from "./TermBlock.svelte";
  import { tilde } from "./term";
  import type { OutputLine, RemoteProbe } from "./types";

  let { url: initialUrl, onDone, onClose }: { url: string; onDone: (path: string) => void; onClose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let url = $state(initialUrl);
  let name = $state("");
  /** The name follows the URL until it is typed over. */
  let nameTouched = $state(false);
  let parent = $state("");
  let submodules = $state(true);
  let shallow = $state(false);
  let probe = $state<{ state: "idle" | "checking" | "ok" | "error"; result?: RemoteProbe; error?: string }>({ state: "idle" });
  let folder = $state<"missing" | "empty" | "files" | "file">("missing");
  let command = $state("");
  let phase = $state<"ask" | "running" | "failed">("ask");
  let output = $state<OutputLine[]>([]);
  let failure = $state("");
  let urlInput = $state<HTMLInputElement>();

  const info = $derived(start.info);
  const home = $derived(info?.home ?? "");

  $effect(() => {
    if (!start.info) start.loadInfo();
    urlInput?.focus();
  });

  // The folder from Settings or the last Choose…, else ~/Developer or the like.
  $effect(() => {
    if (!parent) parent = prefs.get("oxbow.clone.folder") || info?.projects || "";
  });

  $effect(() => {
    const guess = nameFromUrl(url);
    if (!nameTouched) name = guess;
  });

  const target = $derived(parent && name.trim() ? join(parent, name.trim()) : "");
  const options = $derived({ url: url.trim(), path: target, submodules, shallow });

  // Check the URL a moment after typing stops.
  $effect(() => {
    const u = url.trim();
    if (!u) {
      probe = { state: "idle" };
      return;
    }
    probe = { state: "checking" };
    const timer = setTimeout(() => {
      api.probeRemote(u).then(
        (result) => {
          if (url.trim() === u) probe = { state: "ok", result };
        },
        (err) => {
          if (url.trim() === u) probe = { state: "error", error: gitMessage(String(err)) };
        },
      );
    }, 500);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const path = target;
    if (!path) return;
    api.folderState(path).then((state) => {
      if (target === path) folder = state;
    });
  });

  $effect(() => {
    const o = options;
    if (!o.url || !o.path) {
      command = "";
      return;
    }
    api.cloneCommand(o).then(
      (c) => {
        // The shell expands ~, so the command reads as it would be typed.
        if (options === o) command = c.display.endsWith(" " + o.path) ? c.display.slice(0, -o.path.length) + tilde(o.path, home) : c.display;
      },
      () => (command = ""),
    );
  });

  /** What git said, without the command line Oxbow puts in front or git's `fatal:`. */
  function gitMessage(error: string): string {
    const said = error.includes("failed: ") ? error.slice(error.indexOf("failed: ") + 8) : error;
    const lines = said
      .split("\n")
      .map((l) => l.replace(/^(fatal|error): /, "").trim())
      .filter((l) => l && !l.startsWith("Please make sure") && !l.startsWith("and the repository exists"));
    return lines.join(" ") || said.trim();
  }

  const taken = $derived(folder === "files" || folder === "file");
  const invalid = $derived(
    !url.trim() ? "Enter a repository URL" : !name.trim() ? "Name the folder" : /[/\\]/.test(name.trim()) ? "The folder name can't have slashes" : taken ? "That folder is taken" : null,
  );

  async function choose() {
    const path = await chooseFolder({ directory: true, multiple: false, title: "Clone to", defaultPath: parent || undefined });
    if (typeof path !== "string") return;
    parent = path;
    prefs.set("oxbow.clone.folder", path);
  }

  // git's progress, in the three phases the sheet shows.
  type Phase = { label: string; from: RegExp; pct: number; meta: string; started: boolean };
  let phases = $state<Phase[]>([]);
  let cloningInto = $state("");

  function resetPhases() {
    phases = [
      { label: "Receiving objects", from: /^Receiving objects:\s+(\d+)%\s*\(([\d/]+)\)(?:,\s*(.+?))?(?:, done\.)?$/, pct: 0, meta: "", started: false },
      { label: "Resolving deltas", from: /^Resolving deltas:\s+(\d+)%\s*\(([\d/]+)\)/, pct: 0, meta: "", started: false },
      { label: "Checking out files", from: /^Updating files:\s+(\d+)%\s*\(([\d/]+)\)/, pct: 0, meta: "", started: false },
    ];
  }

  function onLine(line: OutputLine) {
    if (!line.progress) output = [...output, line];
    const into = line.text.match(/^Cloning into '(.+)'\.\.\.$/);
    if (into) {
      // A submodule starts its own three phases.
      if (cloningInto) resetPhases();
      cloningInto = into[1];
      return;
    }
    phases.forEach((p, i) => {
      const m = line.text.match(p.from);
      if (!m) return;
      // Earlier phases are done once a later one reports.
      phases.slice(0, i).forEach((before) => {
        before.started = true;
        before.pct = 100;
      });
      p.started = true;
      p.pct = Number(m[1]);
      const [done, all] = m[2].split("/").map(Number);
      p.meta =
        i === 0
          ? [`${p.pct}%`, ...(m[3] ? m[3].split("|").map((s) => s.trim()) : [])].join(" · ")
          : i === 1
            ? `${p.pct}% · ${all.toLocaleString()} deltas`
            : `${done.toLocaleString()} of ${all.toLocaleString()} files`;
    });
  }

  /** The phase git reports on now: the last one that started. */
  const active = $derived(phases.reduce((at, p, i) => (p.started ? i : at), -1));
  const isSubmodule = $derived(!!cloningInto && !!target && cloningInto !== target && cloningInto !== name.trim());

  async function go() {
    if (invalid || phase === "running") return;
    phase = "running";
    output = [];
    failure = "";
    cloningInto = "";
    resetPhases();
    const unlisten = await listen<OutputLine>("clone-line", (event) => onLine(event.payload));
    try {
      const path = await api.cloneRepo(options);
      unlisten();
      onDone(path);
    } catch (err) {
      unlisten();
      const message = String(err);
      if (message === "stopped") {
        phase = "ask";
        return;
      }
      failure = gitMessage(message);
      phase = "failed";
    }
  }

  function close() {
    if (phase === "running") api.stopAction();
    else onClose();
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "Enter" && phase !== "running" && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      go();
    }
  }

  const termLines = $derived.by<TermLine[]>(() => {
    const lines: TermLine[] = command ? [{ kind: "cmd", text: command }] : [];
    if (phase === "failed") {
      for (const line of output.slice(-8)) lines.push({ kind: line.stderr && /^(fatal|error)/.test(line.text) ? "err" : "out", text: line.text });
    }
    return lines;
  });

  const probeBranchColor = 0;
</script>

<svelte:window onkeydown={onKey} />

<div class="dim">
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="clone-title" aria-busy={phase === "running"}>
    <div class="head">
      <span class="title" id="clone-title">Clone Repository</span>
      <span class="sub">Any Git URL works: SSH or HTTPS, GitHub, GitLab, Bitbucket or your own server.</span>
    </div>

    <div class="group">
      <span class="label">Repository URL</span>
      <div class="field">
        <svg class="icon" viewBox="0 0 16 16"><path d="M6.5 9.5a3 3 0 0 0 4.2 0l2.3-2.3a3 3 0 0 0-4.2-4.2L7.6 4.2M9.5 6.5a3 3 0 0 0-4.2 0L3 8.8A3 3 0 0 0 7.2 13l1.2-1.2" /></svg>
        <input
          class="mono"
          bind:this={urlInput}
          bind:value={url}
          placeholder="git@github.com:owner/repo.git"
          aria-label="Repository URL"
          spellcheck="false"
          autocomplete="off"
          disabled={phase === "running"}
        />
      </div>
      {#if probe.state === "checking"}
        <div class="probe">Checking…</div>
      {:else if probe.state === "ok" && probe.result}
        {@const r = probe.result}
        <div class="probe ok">
          <svg viewBox="0 0 16 16"><path d="M3.5 8.5 6.5 11.5 12.5 4.5" /></svg>
          {#if r.transport === "local"}<span>Found on this computer</span>{:else}<span>Reachable over {r.transport}</span>{/if}
          {#if r.defaultBranch}
            <span>· default branch</span>
            <span class="cap" style:background={tint(probeBranchColor, "label")} style:color={plate(probeBranchColor)}>{r.defaultBranch}</span>
            <span class="grey">· {r.branches.toLocaleString()} {r.branches === 1 ? "branch" : "branches"}</span>
          {:else}
            <span class="grey">· empty repository</span>
          {/if}
        </div>
      {:else if probe.state === "error"}
        <div class="probe bad" title={probe.error}>
          <svg viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.5v.1" /></svg>
          <span class="clip">Can't reach it: {probe.error}</span>
        </div>
      {/if}
    </div>

    <div class="group">
      <span class="label">Clone to</span>
      <div class="row">
        <div class="field path">
          <svg class="icon folder" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          <span class="parent" title={parent}>{tilde(parent, home)}{parent.endsWith("/") || parent.endsWith("\\") ? "" : "/"}</span>
          <input
            class="name"
            value={name}
            oninput={(e) => {
              name = e.currentTarget.value;
              nameTouched = true;
            }}
            aria-label="Folder name"
            spellcheck="false"
            autocomplete="off"
            disabled={phase === "running"}
          />
        </div>
        <button class="btn" onclick={choose} disabled={phase === "running"}>Choose…</button>
      </div>
      {#if taken}
        <div class="probe warn">
          <svg viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.5v.1" /></svg>
          <span>{folder === "file" ? "A file has that name here." : `A folder named ${name.trim()} is already here and isn't empty.`} Pick another name.</span>
        </div>
      {/if}
    </div>

    {#if phase === "running"}
      <div class="progress" aria-live="polite">
        {#if isSubmodule}<div class="submodule">Submodule {tilde(cloningInto, home)}</div>{/if}
        {#each phases as p, i (p.label)}
          {@const done = p.pct >= 100 || active > i}
          <div class="phase" class:current={i === active} class:done>
            <span class="mark">
              {#if done}<svg viewBox="0 0 16 16"><path d="M3.5 8.5 6.5 11.5 12.5 4.5" /></svg>{/if}
            </span>
            <span class="phase-label">{p.label}</span>
            {#if i === active}<span class="meta mono">{p.meta}</span>{/if}
          </div>
          {#if i === active}
            <div class="bar"><span class="fill" style:width="{p.pct}%"></span></div>
          {/if}
        {/each}
        {#if active < 0}
          <div class="bar"><span class="fill busy"></span></div>
        {/if}
      </div>
    {:else}
      <div class="options">
        <button class="option" role="switch" aria-checked={submodules} onclick={() => (submodules = !submodules)}>
          <span class="switch" class:on={submodules}><span class="knob"></span></span>
          <span class="words"><span>Include submodules</span><span class="note">Clones and checks out its submodules too</span></span>
        </button>
        <button class="option" role="switch" aria-checked={shallow} onclick={() => (shallow = !shallow)}>
          <span class="switch" class:on={shallow}><span class="knob"></span></span>
          <span class="words"><span>Shallow clone</span><span class="note">Latest commit only. Faster, but History shows just that commit</span></span>
        </button>
      </div>
      {#if phase === "failed"}
        <div class="failed" role="alert">{failure}</div>
      {/if}
      {#if command && (prefs.get("oxbow.confirm.showCommand") || phase === "failed")}
        <TermBlock where="zsh" lines={termLines} copy={command ? [command] : []} />
      {/if}
    {/if}

    <div class="foot">
      <span class="foot-note">
        {#if phase === "running"}Cancelling removes the partial folder.{:else if phase === "failed"}Nothing was left behind.{/if}
      </span>
      {#if phase === "running"}
        <button class="btn" onclick={() => api.stopAction()}>Cancel</button>
      {:else}
        <button class="btn" onclick={onClose}>Cancel</button>
        <button class="btn go" onclick={go} disabled={!!invalid} title={invalid ?? undefined}>{phase === "failed" ? "Try Again" : "Clone"}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .dim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--dim);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 34px;
  }
  .sheet {
    width: min(540px, calc(100vw - 48px));
    max-height: calc(100vh - 68px);
    overflow-y: auto;
    padding: 20px 22px 18px;
    border-radius: 22px;
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.28);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title {
    font-size: 15px;
    font-weight: 700;
  }
  .sub {
    font-size: 12px;
    color: var(--text2);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .row {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 10px;
    border-radius: 10px;
    background: var(--field);
    border: 0.5px solid var(--glass-border);
    color: var(--text2);
    flex-grow: 1;
    min-width: 0;
  }
  .field:focus-within {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .field input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
  }
  .field input.mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .folder {
    color: var(--accent);
    flex-shrink: 0;
  }
  .path {
    gap: 6px;
  }
  .parent {
    flex-shrink: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text2);
  }
  .field .name {
    flex: 1 0 90px;
    margin-left: -6px;
  }
  .probe {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text2);
    min-width: 0;
  }
  .probe svg {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .probe.ok {
    color: var(--green);
  }
  .probe.bad {
    color: var(--red);
  }
  .probe.warn {
    color: var(--orange);
  }
  .clip {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grey {
    color: var(--text2);
  }
  .cap {
    display: flex;
    align-items: center;
    height: 16px;
    padding: 0 7px;
    border-radius: 8px;
    font-weight: 500;
  }
  .options {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    text-align: left;
  }
  .switch {
    width: 30px;
    height: 18px;
    border-radius: 9px;
    background: var(--switch-off);
    position: relative;
    flex-shrink: 0;
    margin-top: 1px;
    transition: background 0.15s;
  }
  .switch.on {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 7px;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: left 0.15s;
  }
  .switch.on .knob {
    left: 14px;
  }
  .words {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .note {
    font-size: 11px;
    color: var(--text2);
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
    border-radius: 14px;
    background: var(--field);
  }
  .submodule {
    font-size: 11px;
    color: var(--text2);
  }
  .phase {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text2);
  }
  .phase.current {
    color: var(--text);
    font-weight: 600;
  }
  .mark {
    width: 14px;
    height: 14px;
    border-radius: 7px;
    border: 1px solid color-mix(in srgb, var(--text2) 45%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .current .mark {
    border-color: var(--lane-0);
  }
  .done .mark {
    background: var(--lane-0);
    border-color: var(--lane-0);
    color: #fff;
  }
  .mark svg {
    width: 9px;
    height: 9px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .phase-label {
    flex-grow: 1;
  }
  .meta {
    font-size: 11px;
    font-weight: 400;
    color: var(--text2);
  }
  .bar {
    height: 5px;
    margin-left: 22px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--text2) 18%, transparent);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--lane-0);
    transition: width 0.2s;
  }
  .fill.busy {
    width: 30%;
    animation: slide 1.2s ease-in-out infinite alternate;
  }
  @keyframes slide {
    from {
      transform: translateX(-60%);
    }
    to {
      transform: translateX(300%);
    }
  }
  .failed {
    font-size: 12px;
    line-height: 1.4;
    color: var(--red);
    overflow-wrap: anywhere;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 2px;
  }
  .foot-note {
    flex-grow: 1;
    font-size: 11px;
    color: var(--text2);
  }
  .btn {
    flex-shrink: 0;
    min-width: 84px;
    height: 30px;
    padding: 0 16px;
    border-radius: 15px;
    text-align: center;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>
