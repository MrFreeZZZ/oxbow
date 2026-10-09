<script lang="ts">
  // Sign in to GitHub: in the browser with a one-time code (GitHub's device flow, as
  // `gh auth login --web` does), or with a personal access token pasted here. Either way the
  // token goes to the computer's credential store and the requests show in the terminal block.

  import { onDestroy } from "svelte";
  import { api } from "./api";
  import { prefs } from "./prefs.svelte";
  import TermBlock, { type TermLine } from "./TermBlock.svelte";
  import type { DeviceCode, GitHubAccount, SignInSetup } from "./types";

  let { onDone, onClose, tokenFirst = false }: { onDone: (account: GitHubAccount) => void; onClose: () => void; tokenFirst?: boolean } = $props();

  let setup = $state<SignInSetup | null>(null);
  /** browser: the code to approve; token: a pasted token. */
  let mode = $state<"browser" | "token">("browser");
  let phase = $state<"ask" | "starting" | "waiting" | "checking">("ask");
  let code = $state<DeviceCode | null>(null);
  let token = $state("");
  let problem = $state<string | null>(null);
  let copied = $state(false);
  let tokenInput = $state<HTMLInputElement>();

  api.githubSignInSetup().then((s) => {
    setup = s;
    // svelte-ignore state_referenced_locally
    if (!s.clientId || tokenFirst) mode = "token";
  });

  $effect(() => {
    if (mode === "token") tokenInput?.focus();
  });

  /** Counts sign-ins started and cancelled: one that finds it changed after an await stops there. */
  let attempt = 0;

  async function startBrowser() {
    const mine = ++attempt;
    problem = null;
    phase = "starting";
    let started: DeviceCode;
    try {
      started = await api.githubDeviceStart();
    } catch (err) {
      if (mine !== attempt) return;
      problem = String(err);
      phase = "ask";
      return;
    }
    if (mine !== attempt) return;
    code = started;
    phase = "waiting";
    await copyAndOpen();
    if (mine !== attempt) return;
    try {
      const account = await api.githubDeviceWait(started.deviceCode, started.interval, started.expiresIn);
      if (mine === attempt) onDone(account);
    } catch (err) {
      if (mine !== attempt) return;
      if (String(err) !== "stopped") problem = String(err);
      phase = "ask";
      code = null;
    }
  }

  async function copyAndOpen() {
    if (!code) return;
    await navigator.clipboard.writeText(code.userCode).catch(() => {});
    copied = true;
    setTimeout(() => (copied = false), 2000);
    api.openGitHub(code.verificationUri).catch((err) => (problem = String(err)));
  }

  async function signInWithToken() {
    const mine = ++attempt;
    problem = null;
    phase = "checking";
    try {
      const account = await api.githubSignInToken(token);
      if (mine === attempt) onDone(account);
    } catch (err) {
      if (mine !== attempt || String(err) === "stopped") return;
      problem = String(err);
      phase = "ask";
    }
  }

  /** Stop whatever sign-in is under way: nothing it brings back is kept or opened. */
  function stop() {
    if (phase === "ask") return;
    attempt += 1;
    api.githubDeviceStop().catch(() => {});
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
    } else if (event.key === "Enter" && mode === "token" && token.trim() && phase === "ask") {
      event.preventDefault();
      signInWithToken();
    }
  }

  const lines = $derived.by((): TermLine[] => {
    if (!setup) return [];
    if (mode === "browser")
      return [
        { kind: "comment", text: "what gh auth login --web does: ask for a one-time code" },
        { kind: "cmd", text: setup.deviceRequests[0] },
        { kind: "comment", text: "then, every few seconds, until it is approved in the browser" },
        { kind: "cmd", text: setup.deviceRequests[1] },
      ];
    return [
      { kind: "comment", text: "check the token: who it belongs to and its scopes" },
      { kind: "cmd", text: setup.tokenRequest },
    ];
  });
  const scopeList = $derived((setup?.scopes ?? []).join(", "));
  const copy = $derived(lines.filter((l) => l.kind === "cmd").map((l) => l.text));
</script>

<svelte:window onkeydown={onKey} />

<div class="dim">
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="github-title" aria-busy={phase !== "ask"}>
    <div class="head">
      <span class="mark" aria-hidden="true">
        <svg viewBox="0 0 16 16"><path d="M8 1.3a6.7 6.7 0 0 0-2.1 13.1c.3 0 .5-.2.5-.4v-1.3c-1.9.4-2.3-.9-2.3-.9-.3-.8-.7-1-.7-1-.6-.4 0-.4 0-.4.7 0 1 .7 1 .7.6 1 1.6.7 2 .5 0-.4.2-.7.4-.9-1.5-.2-3-.7-3-3.3 0-.7.2-1.3.7-1.8-.1-.2-.3-.9.1-1.8 0 0 .6-.2 1.8.7a6 6 0 0 1 3.3 0c1.3-.9 1.8-.7 1.8-.7.4.9.2 1.6.1 1.8.4.5.7 1.1.7 1.8 0 2.6-1.6 3.1-3 3.3.2.2.4.6.4 1.2V14c0 .2.2.5.5.4A6.7 6.7 0 0 0 8 1.3" /></svg>
      </span>
      <span class="words">
        <span class="title" id="github-title">Sign in to GitHub</span>
        <span class="sub">To publish repositories and work with pull requests. The token is kept in the {setup?.store ?? "Keychain"}, never in a file.</span>
      </span>
    </div>

    {#if setup?.clientId}
      <div class="seg" role="radiogroup" aria-label="How to sign in">
        <button role="radio" aria-checked={mode === "browser"} disabled={phase !== "ask"} onclick={() => ((mode = "browser"), (problem = null))}>In the browser</button>
        <button role="radio" aria-checked={mode === "token"} disabled={phase !== "ask"} onclick={() => ((mode = "token"), (problem = null))}>With a token</button>
      </div>
    {/if}

    {#if mode === "browser"}
      {#if code}
        <div class="code-box">
          <span class="label">Enter this code on github.com/login/device</span>
          <span class="code mono selectable" aria-label="One-time code">{code.userCode}</span>
          <button class="btn" onclick={copyAndOpen}>{copied ? "Copied · opening GitHub…" : "Copy Code and Open GitHub"}</button>
          <span class="waiting"><span class="spinner"></span>Waiting for you to approve Oxbow in the browser…</span>
        </div>
      {:else}
        <p class="text">
          GitHub opens in your browser with a one-time code to approve. Oxbow asks for the scopes <code>{scopeList}</code>: private repositories, workflow files and your organizations.
        </p>
      {/if}
    {:else}
      <div class="group">
        <span class="label">Personal access token</span>
        <div class="field">
          <input
            class="mono"
            type="password"
            bind:this={tokenInput}
            bind:value={token}
            placeholder="ghp_… or github_pat_…"
            aria-label="Personal access token"
            spellcheck="false"
            autocomplete="off"
            disabled={phase !== "ask"}
          />
        </div>
        <span class="note">
          A classic token needs the scopes <code>{scopeList}</code>. A fine-grained one needs Contents, Pull requests and Administration (to create repositories) set to Read and write.
          <button class="link" onclick={() => setup && api.openGitHub(setup.newTokenUrl)}>Create a token on GitHub</button>
        </span>
        {#if !setup?.clientId && setup}
          <span class="note">Signing in with the browser needs Oxbow’s GitHub app, which isn’t set up in this build yet.</span>
        {/if}
      </div>
    {/if}

    {#if problem}<div class="failed" role="alert">{problem}</div>{/if}

    {#if lines.length && prefs.get("oxbow.confirm.showCommand")}
      <TermBlock where="zsh" {lines} {copy} />
    {/if}

    <div class="foot">
      <span class="foot-note"></span>
      <button class="btn" onclick={cancel}>Cancel</button>
      {#if mode === "browser"}
        {#if !code}
          <button class="btn go" onclick={startBrowser} disabled={phase !== "ask"}>{phase === "starting" ? "Asking GitHub…" : "Continue in Browser"}</button>
        {/if}
      {:else}
        <button class="btn go" onclick={signInWithToken} disabled={!token.trim() || phase !== "ask"}>{phase === "checking" ? "Checking…" : "Sign In"}</button>
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
    gap: 12px;
    align-items: flex-start;
  }
  .mark {
    width: 38px;
    height: 38px;
    border-radius: 11px;
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
    font-size: 15px;
    font-weight: 700;
  }
  .sub,
  .text {
    font-size: 12px;
    line-height: 1.45;
    color: var(--text2);
    margin: 0;
  }
  code {
    font-family: var(--mono);
    font-size: 11px;
  }
  .seg {
    display: flex;
    align-self: flex-start;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
    border: 0.5px solid var(--glass-border);
  }
  .seg button {
    height: 24px;
    padding: 0 14px;
    border-radius: 7px;
    font-size: 12px;
    color: var(--text2);
  }
  .seg button[aria-checked="true"] {
    background: var(--glass);
    color: var(--text);
    font-weight: 600;
    box-shadow: var(--glass-shadow);
  }
  .code-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 16px;
    border-radius: 14px;
    background: var(--field);
  }
  .code {
    font-size: 28px;
    font-weight: 600;
    letter-spacing: 0.12em;
  }
  .waiting {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--text2);
  }
  .spinner {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--text2) 30%, transparent);
    border-top-color: var(--text2);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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
  .field {
    display: flex;
    align-items: center;
    height: 34px;
    padding: 0 10px;
    border-radius: 10px;
    background: var(--field);
    border: 0.5px solid var(--glass-border);
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
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
  }
  .note {
    font-size: 11px;
    line-height: 1.45;
    color: var(--text2);
  }
  .link {
    color: var(--accent);
    font-size: 11px;
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
