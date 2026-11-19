import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { addLine, cutLine } from "../src/lib/liveLines.ts";
import type { Request, confirm } from "../src/lib/confirm.svelte.ts";
import type { Action, Plan } from "../src/lib/types.ts";

const source = fs.readFileSync(new URL("../src/lib/confirm.svelte.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const compiled = compileModule(ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "confirm.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function fixture() {
  const plans = new Map<string, ReturnType<typeof Promise.withResolvers<Plan>>>();
  const performed: Action[] = [];
  const execution = Promise.withResolvers<string>();
  const globals = {
    $: runtime,
    getCurrentWindow: () => ({ listen: async () => () => {} }),
    prefs: { get: () => false },
    addLine,
    cutLine,
    api: {
      planAction: (action: Extract<Action, { kind: "renameBranch" }>) => {
        const plan = Promise.withResolvers<Plan>();
        plans.set(action.to, plan);
        return plan.promise;
      },
      performAction: (action: Action) => {
        performed.push(action);
        return execution.promise;
      },
    },
  };
  const sheet: typeof confirm = Function(...Object.keys(globals), `${compiled}; return confirm;`)(...Object.values(globals));
  const reply = async (name: string) => {
    const display = `git branch -m old ${name}`;
    plans.get(name)!.resolve({ commands: [{ display, args: [], comment: null, input: null, before: null, todo: null }], notKept: [] });
    await Promise.resolve();
  };
  return { sheet, plans, performed, execution, reply };
}

const rename = (to: string): Request => ({
  title: "Rename old?", body: [], button: "Rename", icon: "edit", always: true,
  action: { kind: "renameBranch", from: "old", to, upstream: null },
});

test("only the latest rename plan enables confirmation, including during the typing pause", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { sheet, performed, execution, reply } = fixture();
  const done = sheet.run(rename("initial"));
  assert.equal(sheet.phase, "ask");
  assert.equal(sheet.canGo, false);
  sheet.go();
  assert.equal(performed.length, 0);
  await reply("initial");
  assert.equal(sheet.phase, "ask");
  assert.equal(sheet.canGo, true);

  sheet.edit(rename("first"));
  assert.equal(sheet.phase, "ask");
  sheet.go();
  assert.equal(sheet.canGo, false);
  assert.equal(performed.length, 0);
  t.mock.timers.tick(150);
  sheet.edit(rename("second"));
  await reply("first");
  assert.equal(sheet.commands[0].display, "git branch -m old initial");
  assert.equal(sheet.canGo, false);

  t.mock.timers.tick(150);
  sheet.edit(rename("latest"));
  t.mock.timers.tick(150);
  sheet.go();
  assert.equal(performed.length, 0);
  await reply("latest");
  assert.equal(sheet.commands[0].display, "git branch -m old latest");
  assert.equal(sheet.canGo, true);
  sheet.go();
  await reply("second");
  assert.equal(sheet.phase, "running");
  assert.equal(sheet.commands[0].display, "git branch -m old latest");
  assert.deepEqual(performed, [rename("latest").action]);

  execution.resolve("");
  assert.equal(await done, true);
});

test("a changed local plan supersedes pending edits and choices, and a cancelled Git plan cannot touch a new sheet", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { sheet, plans, performed, reply } = fixture();
  const done = sheet.run(rename("initial"));
  await reply("initial");

  const choice = Promise.withResolvers<Request>();
  const changing = sheet.change(choice.promise);
  sheet.go();
  assert.equal(sheet.canGo, false);
  assert.equal(performed.length, 0);
  sheet.edit(rename("discarded"));
  const local: Request = {
    title: "Save?", body: [], icon: "edit", button: "Save", always: true,
    local: { commands: ["save"], run: async () => "Saved." },
  };
  await sheet.change(local);
  choice.resolve(rename("old-choice"));
  await changing;
  t.mock.timers.tick(150);
  assert.equal(plans.has("discarded") || plans.has("old-choice"), false);
  assert.equal(sheet.canGo, true);
  assert.equal(sheet.commands[0].display, "save");

  sheet.edit(rename("cancelled"));
  t.mock.timers.tick(150);
  sheet.cancel();
  assert.equal(await done, false);
  const next = sheet.run(local);
  plans.get("cancelled")!.reject(new Error("late failure"));
  await Promise.resolve();
  assert.equal(sheet.phase, "ask");
  assert.equal(sheet.canGo, true);
  assert.equal(sheet.commands[0].display, "save");
  sheet.go();
  assert.equal(await next, true);
});

test("a failure of the current Git plan uses the existing error sheet", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { sheet, plans, performed, reply } = fixture();
  const done = sheet.run(rename("initial"));
  await reply("initial");
  sheet.edit(rename("failed"));
  t.mock.timers.tick(150);
  plans.get("failed")!.reject(new Error("plan failed"));
  await Promise.resolve();
  assert.equal(sheet.phase, "failed");
  assert.equal(sheet.planning, false);
  assert.equal(sheet.canGo, false);
  assert.deepEqual(sheet.commands, []);
  assert.match(sheet.lines[0].text, /plan failed/);
  sheet.go();
  assert.equal(performed.length, 0);
  sheet.cancel();
  assert.equal(await done, false);
});
