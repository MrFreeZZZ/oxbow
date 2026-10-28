<script lang="ts">
  // Sign in to GitLab with a personal access token: gitlab.com or a server of your own, one
  // account each. GitLab says whose the token is, its scopes and the server's version before
  // Oxbow keeps it in the computer's credential store; the requests show in the terminal block.

  import { modal } from "./modal";
  import { onDestroy, onMount } from "svelte";
  import { api } from "./api";
  import { prefs } from "./prefs.svelte";
  import TermBlock, { type TermLine } from "./TermBlock.svelte";
  import type { GitLabAccount, GitLabSignInSetup } from "./types";

  let { server: start = "gitlab.com", onDone, onClose }: { server?: string; onDone: (account: GitLabAccount) => void; onClose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let server = $state(start.replace(/^https:\/\//, ""));
  let token = $state("");
  let setup = $state<GitLabSignInSetup | null>(null);
  /** The server typed isn't an address. */
  let badServer = $state<string | null>(null);
  let checking = $state(false);
  let problem = $state<string | null>(null);
  let serverInput = $state<HTMLInputElement>();
  let tokenInput = $state<HTMLInputElement>();

  // The requests and the token page follow the server as it is typed.
  $effect(() => {
    const typed = server;
    api.gitlabSignInSetup(typed).then(
      (s) => {
        if (typed !== server) return;
        setup = s;
        badServer = null;
      },
      (err) => typed === server && (badServer = String(err)),
    );
  });

  // The server when none is given yet, else the token.
  onMount(() => (server ? tokenInput : serverInput)?.focus());

  /** Counts sign-ins started and cancelled: one that finds it changed after an await stops there. */
  let attempt = 0;

  async function signIn() {
    if (badServer || !token.trim() || checking) return;
    const mine = ++attempt;
    problem = null;
    checking = true;
    try {
      const account = await api.gitlabSignIn(server, token);
      if (mine === attempt) onDone(account);
    } catch (err) {
      if (mine !== attempt || String(err) === "stopped") return;
      problem = String(err);
    } finally {
      if (mine === attempt) checking = false;
    }
  }

  function stop() {
    if (!checking) return;
    attempt += 1;
    api.gitlabSignInStop().catch(() => {});
  }

  function cancel() {
    stop();
    onClose();
  }

  onDestroy(stop);

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
    } else if (event.key === "Enter" && token.trim() && !checking) {
      event.preventDefault();
      signIn();
    }
  }

  const host = $derived(setup?.server.host ?? "gitlab.com");
  const lines = $derived.by((): TermLine[] =>
    setup
      ? [
          { kind: "comment", text: "check whose token it is, the server's version and the token's scopes" },
          ...setup.requests.map((text): TermLine => ({ kind: "cmd", text })),
        ]
      : [],
  );
  const copy = $derived(lines.filter((l) => l.kind === "cmd").map((l) => l.text));
</script>

<svelte:window onkeydown={onKey} />

<div class="dim">
  <div class="sheet" use:modal role="dialog" aria-modal="true" aria-labelledby="gitlab-title" aria-busy={checking}>
    <div class="head">
      <span class="mark" aria-hidden="true">
        <svg viewBox="0 0 16 16"><path d="m14.6 6.4-.02-.05-1.8-4.7a.47.47 0 0 0-.9.05l-1.2 3.7H5.3L4.1 1.7a.47.47 0 0 0-.9-.05l-1.8 4.7-.02.05a3.3 3.3 0 0 0 1.1 3.8l.01.01 2.7 2 1.3 1 .8.6a.55.55 0 0 0 .66 0l.8-.6 1.3-1 2.7-2 .01-.01a3.3 3.3 0 0 0 1.1-3.8" /></svg>
      </span>
      <span class="words">
        <span class="title" id="gitlab-title">Sign in to GitLab</span>
        <span class="sub">To work with merge requests on gitlab.com or a server of your own, GitLab {setup?.oldest ?? "16.0"} or later. The token is kept in the {setup?.store ?? "Keychain"}, never in a file.</span>
      </span>
    </div>

    <div class="group">
      <span class="label">Server</span>
      <div class="field">
        <input bind:this={serverInput} bind:value={server} placeholder="gitlab.com" aria-label="Server" spellcheck="false" autocomplete="off" disabled={checking} />
      </div>
      {#if badServer}
        <span class="note bad">{badServer}</span>
      {:else}
        <span class="note">As in the browser: gitlab.com, git.example.com or https://git.example.com:8443</span>
      {/if}
    </div>

    <div class="steps">
      <div class="steps-head">
        <span class="label">Get a token on {host}</span>
        <button class="button small" disabled={!!badServer} onclick={() => api.gitlabOpenNewToken(server).catch((err) => (problem = String(err)))}>Open Token Page</button>
      </div>
      <ol>
        <li>The page opens with the name Oxbow and the <code>{(setup?.scopes ?? ["api"]).join(", ")}</code> scope already ticked.</li>
        <li>Pick an expiry date, up to a year. Oxbow reminds you two weeks before it.</li>
        <li>Press <b>Create personal access token</b>, copy the token that starts with <code>glpat-</code> and paste it below. GitLab shows it only once.</li>
      </ol>
      <span class="note">No such page on {host}? It is under your avatar › Edit profile › Access tokens there.</span>
    </div>

    <div class="group">
      <span class="label">Personal access token</span>
      <div class="field">
        <input
          class="mono"
          type="password"
          bind:this={tokenInput}
          bind:value={token}
          placeholder="glpat-…"
          aria-label="Personal access token"
          spellcheck="false"
          autocomplete="off"
          disabled={checking}
        />
      </div>
      <span class="note">Only Oxbow’s calls to GitLab use it; fetch and push keep your SSH keys and passwords.</span>
    </div>

    {#if problem}<div class="failed" role="alert">{problem}</div>{/if}

    {#if lines.length && prefs.get("oxbow.confirm.showCommand")}
      <TermBlock where="zsh" {lines} {copy} />
    {/if}

    <div class="foot">
      <span class="foot-note"></span>
      <button class="button" onclick={cancel}>Cancel</button>
      <button class="button primary" onclick={signIn} disabled={!token.trim() || !!badServer || checking}>{checking ? `Asking ${host}…` : "Sign In"}</button>
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
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .mark {
    width: 38px;
    height: 38px;
    border-radius: var(--radius-l);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--lane-0) 16%, transparent);
    color: var(--lane-0);
  }
  .mark svg {
    width: 22px;
    height: 22px;
    fill: currentColor;
  }
  .words {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title {
    font-size: var(--text-heading);
    font-weight: 700;
  }
  .sub {
    font-size: var(--text-small);
    line-height: 1.45;
    color: var(--text2);
  }
  code {
    font-family: var(--mono);
    font-size: var(--text-caption);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .field {
    display: flex;
    align-items: center;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    background: var(--field);
    border: 0.5px solid var(--glass-border);
  }
  .field:focus-within {
    box-shadow: var(--focus-ring);
  }
  .field input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font-size: var(--text-small);
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
    box-shadow: none;
  }
  .field input.mono {
    font-family: var(--mono);
  }
  .note {
    font-size: var(--text-caption);
    line-height: 1.45;
    color: var(--text2);
  }
  .note.bad {
    color: var(--red);
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-l);
    background: var(--field);
  }
  .steps-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .steps ol {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-small);
    line-height: 1.45;
  }
  .steps b {
    font-weight: 600;
  }
  .button.small {
    min-width: 0;
    height: 24px;
    padding: 0 10px;
    font-size: var(--text-caption);
  }
  .failed {
    font-size: var(--text-small);
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
  }

  .button {
    min-width: 84px;
    padding: 0 16px;
  }
</style>
