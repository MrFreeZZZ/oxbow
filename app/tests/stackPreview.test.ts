import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { api, apiForSession, stopped, type RepoNotice } from "../src/lib/api.ts";

const view = fs.readFileSync(new URL("../src/lib/EditStackView.svelte", import.meta.url), "utf8");
const shared = view.slice(view.indexOf("  let pendingPreview"), view.indexOf("</script>"));
const preview = view.slice(view.indexOf("  // The dry run follows"), view.indexOf("  const byId"));
const js = ts.transpile(`
  ${shared}
  function mount(workdir) {
    let stack = {};
    let loading = false;
    let draft = $state("initial");
    let preview = $state(null);
    let previewError = $state(null);
    let checking = $state(false);
    const planOf = (_, draft) => draft;
    ${preview}
    return {
      change: (plan) => { draft = plan; },
      get result() { return preview; },
      get error() { return previewError; },
      get checking() { return checking; },
    };
  }
`, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext });
const compiled = compileModule(js, { filename: "stackPreview.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test("Edit Stack cancels immediately and lets only the latest preview run and respond", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const previews: { plan: string; scope?: { workdir: string; lane: string; onStart: { onmessage: () => void } }; reply: ReturnType<typeof Promise.withResolvers<string>> }[] = [];
  const stops: { lanes: string[]; reply: ReturnType<typeof Promise.withResolvers<void>> }[] = [];
  const invoke = (command: string, args: any) => {
    if (command === "stop_reads") {
      const reply = Promise.withResolvers<void>();
      stops.push({ lanes: args.lanes, reply });
      return reply.promise;
    }
    assert.equal(command, "stack_preview");
    const reply = Promise.withResolvers<string>();
    previews.push({ ...args, reply });
    return reply.promise;
  };
  Object.defineProperty(globalThis, "window", { configurable: true, value: { __TAURI_INTERNALS__: {
    invoke, transformCallback: () => 0,
  } } });
  t.after(() => { Reflect.deleteProperty(globalThis, "window"); });
  const mount = Function("$", "api", "stopped", `${compiled}; return mount;`)(runtime, api, stopped);
  const open = (workdir: string) => {
    let state!: ReturnType<typeof mount>;
    const close = runtime.effect_root(() => { state = mount(workdir); });
    runtime.flush();
    return { state, close };
  };
  const tick = async (ms: number) => {
    t.mock.timers.tick(ms);
    await settle();
  };

  open("/first").close();
  await tick(250);
  assert.equal(previews.length, 0);

  const editor = open("/first");
  await tick(250);
  assert.equal(previews[0].plan, "initial");
  editor.state.change("superseded");
  runtime.flush();
  assert.equal(stops.length, 0, "the Stop must wait until the lane exists");
  previews[0].scope!.onStart.onmessage();
  assert.deepEqual(stops[0].lanes, [previews[0].scope!.lane]);
  editor.state.change("latest");
  runtime.flush();
  await tick(250);
  assert.equal(previews.length, 1, "the next plan waits for Git to exit");
  previews[0].reply.resolve("old result");
  await settle();
  assert.equal(editor.state.result, null);
  assert.equal(editor.state.checking, true);
  assert.equal(previews.length, 1, "the pending Stop cannot hit a replacement");
  stops[0].reply.resolve();
  await settle();
  assert.equal(previews[1].plan, "latest");
  previews[1].scope!.onStart.onmessage();
  editor.close();
  assert.equal(stops.length, 2, "closing after Git starts cancels it immediately");
  const reopened = open("/second");
  t.after(reopened.close);
  await tick(250);
  assert.equal(previews.length, 2);
  previews[1].reply.reject("stopped");
  stops[1].reply.resolve();
  await settle();
  assert.equal(previews[2].scope!.workdir, "/second");
  previews[2].reply.resolve("current result");
  await settle();
  assert.equal(reopened.state.result, "current result");
  assert.equal(reopened.state.error, null);
  assert.equal(reopened.state.checking, false);

  const restack = api.stackPreview("restack" as unknown as Parameters<typeof api.stackPreview>[0]);
  assert.equal(previews[3].scope, undefined);
  previews[3].reply.resolve("restack result");
  assert.equal(await restack, "restack result");
});

test("tab calls and the delayed stack Stop retain their IPC address", async (t) => {
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  const preview = Promise.withResolvers<string>();
  let started: (() => void) | undefined;
  Object.defineProperty(globalThis, "window", { configurable: true, value: { __TAURI_INTERNALS__: {
    transformCallback: () => 0,
    invoke(command: string, args?: Record<string, unknown>) {
      calls.push({ command, args });
      if (command === "stack_preview") {
        const scope = args?.scope as { onStart: { onmessage: () => void } };
        started = () => scope.onStart.onmessage();
        return preview.promise;
      }
      return Promise.resolve();
    },
  } } });
  t.after(() => { Reflect.deleteProperty(globalThis, "window"); });

  const notices: RepoNotice[] = [];
  await api.repoEvents((notice) => notices.push(notice));
  const onNotice = calls[0].args?.onNotice as { onmessage: (notice: RepoNotice) => void };
  onNotice.onmessage({ session: 1, event: "repo-activate-request", payload: null });

  const a = apiForSession(1);
  const b = apiForSession(2);
  const controller = new AbortController();
  const plan = {} as Parameters<typeof a.stackPreview>[0];
  const result = a.stackPreview(plan, { workdir: "/a", signal: controller.signal });
  await b.activateRepoTab();
  controller.abort();
  started?.();
  await b.stopFetch(1);
  await api.stopAction();
  preview.reject("stopped");
  await assert.rejects(result, (error) => stopped(error));

  assert.deepEqual(calls.map(({ command, args }) => [command, args?.repoSession]), [
    ["repo_events", undefined],
    ["stack_preview", 1],
    ["activate_repo_tab", 2],
    ["stop_reads", 1],
    ["stop_fetch", 2],
    ["stop_action", undefined],
  ]);
  assert.deepEqual(notices, [{ session: 1, event: "repo-activate-request", payload: null }]);
});
