// Runs the scenarios of scenarios.mjs one after another (scripts/acceptance.sh sets it up):
// one line each, and on a failure a picture of the window and the error in target/acceptance/.
// `--speed <folder>` runs the timings of speed.mjs instead, on scripts/make-perf-fixtures.sh's
// repositories in that folder.

import fs from "node:fs";
import path from "node:path";
import { App } from "./driver.mjs";
import { SCENARIOS } from "./scenarios.mjs";
import { SPEED } from "./speed.mjs";

const binary = process.env.OXBOW_APP;
const home = process.env.OXBOW_HOME;
const out = process.env.OXBOW_OUT ?? ".";
if (!binary || !home) {
  console.error("run.mjs: start it with scripts/acceptance.sh");
  process.exit(2);
}
const config = path.join(process.env.XDG_CONFIG_HOME, "app.oxbow.desktop");
const data = path.join(process.env.XDG_DATA_HOME, "app.oxbow.desktop");

let words = process.argv.slice(2);
let perf = null;
if (words[0] === "--speed") {
  perf = path.resolve(words[1] ?? "");
  words = words.slice(2);
  if (!fs.existsSync(path.join(perf, "history-100k"))) {
    console.error(`run.mjs: no fixtures in ${perf}: scripts/make-perf-fixtures.sh ${perf}`);
    process.exit(2);
  }
}
const chosen = (perf ? SPEED : SCENARIOS).filter((s) => !words.length || words.some((w) => s.name.includes(w)));
let failed = 0;

for (const scenario of chosen) {
  // Each scenario starts as on a new Mac: no settings, no recent repositories, no drafts.
  fs.rmSync(config, { recursive: true, force: true });
  fs.rmSync(data, { recursive: true, force: true });
  const dir = fs.mkdtempSync(path.join(home, "repos-"));
  let app = null;
  const ctx = {
    dir,
    perf,
    config,
    data,
    /** Starts the app, with `repo` as the repository it reopens, and waits for its history. */
    async start(repo) {
      if (app) await app.quit();
      app = null;
      if (repo) {
        fs.mkdirSync(config, { recursive: true });
        fs.writeFileSync(path.join(config, "last-repository"), repo);
      }
      // tauri-driver can still be closing the last run's app: a moment, then again.
      for (let tries = 0; ; tries++) {
        try {
          app = await App.start(binary);
          break;
        } catch (err) {
          if (tries === 3) throw err;
          await new Promise((r) => setTimeout(r, 1000));
        }
      }
      if (repo) await app.js((page) => page.until(() => page.all("[aria-label=Commits] [role=option]").length, "the history"));
      return app;
    },
    get app() {
      return app;
    },
  };
  const began = Date.now();
  try {
    const note = await scenario.run(ctx);
    console.log(`✓ ${scenario.name} (${((Date.now() - began) / 1000).toFixed(1)} s)${note ? ` — ${note}` : ""}`);
  } catch (err) {
    failed++;
    const name = scenario.name.replace(/[^a-z0-9]+/gi, "-");
    console.log(`✗ ${scenario.name}: ${String(err.message).split("\n")[0]}`);
    fs.writeFileSync(path.join(out, `${name}.txt`), String(err.stack));
    if (app) await app.screenshot().then((png) => fs.writeFileSync(path.join(out, `${name}.png`), png)).catch(() => {});
  } finally {
    if (app) await app.quit();
  }
}

console.log(failed ? `acceptance: ${failed} of ${chosen.length} failed (target/acceptance/)` : `acceptance: all ${chosen.length} passed`);
process.exit(failed ? 1 : 0);
