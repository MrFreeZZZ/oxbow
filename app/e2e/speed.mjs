// What Oxbow's own window takes, from the user's side: opening a big repository, a refresh after
// a commit made outside, and moving between commits. `scripts/acceptance.sh --speed <folder>`
// runs them on scripts/make-perf-fixtures.sh's repositories; docs/speed.md has the numbers. The
// same three are timed against other clients on a Mac with scripts/compare-clients.py.

import path from "node:path";
import { git } from "./fixtures.mjs";

const RUNS = Number(process.env.RUNS ?? 15);

/** "median / p95 ms (n)": p95 by nearest rank, so it needs about 20 runs to mean anything. */
function stats(times) {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)];
  return `${at(0.5)} / ${at(0.95)} ms (${times.length} runs)`;
}

/** Until the history list shows rows, from now. */
const historyShown = (app) => app.js((page) => page.until(() => page.all("[aria-label=Commits] [role=option]").length, "the history", 60_000));

/** Times from a real key press to the next commit's diff on screen: the page notes when the key
 *  came, and waits until the details show the selected commit with its diff painted. */
async function step(app, key) {
  await app.js(async (page) => {
    window.__keyAt = null;
    window.addEventListener("keydown", () => (window.__keyAt ??= performance.now()), { capture: true, once: true });
  });
  await app.keys(key);
  return app.js(async (page) => {
    await page.until(() => window.__keyAt !== null, "the key");
    // The fixture's commit «Change file k (i)» writes "line i".
    const shown = () => {
      const row = page.all("[aria-label=Commits] [aria-selected=true]")[0];
      const i = row && /\((\d+)\)/.exec(page.words(row))?.[1];
      return i && [...document.querySelectorAll(".line .text")].some((t) => t.textContent.trim() === `line ${i}`);
    };
    await page.until(shown, "the commit's diff", 30_000);
    await page.paint();
    return Math.round(performance.now() - window.__keyAt);
  });
}

export const SPEED = [
  {
    name: "open 100k",
    // From starting the app to the 100,000-commit history of a clone on screen; the app
    // reopens it.
    async run(ctx) {
      const repo = path.join(ctx.perf, "history-100k-clone");
      const times = [];
      for (let i = 0; i < RUNS; i++) {
        const t = Date.now();
        await ctx.start(repo);
        times.push(Date.now() - t);
      }
      return `start to history: ${stats(times)}`;
    },
  },
  {
    name: "first diff",
    // The first commit picked after opening: its diff, with nothing read before it.
    async run(ctx) {
      const repo = path.join(ctx.perf, "history-100k-clone");
      const times = [];
      for (let i = 0; i < Math.min(RUNS, 10); i++) {
        const app = await ctx.start(repo);
        await app.tap((page) => page.all("[aria-label=Commits] [role=option]")[0]);
        await app.js((page) => page.sleep(1500));
        times.push(await step(app, "ArrowDown"));
      }
      return `key to diff: ${stats(times)}`;
    },
  },
  {
    name: "next commit",
    // ↓ through the history (each commit read the first time), then ↑ back (read before).
    async run(ctx) {
      const app = await ctx.start(path.join(ctx.perf, "history-100k-clone"));
      await app.tap((page) => page.all("[aria-label=Commits] [role=option]")[0]);
      await app.js((page) => page.sleep(1500));
      const down = [];
      const up = [];
      for (let i = 0; i < RUNS; i++) down.push(await step(app, "ArrowDown"));
      for (let i = 0; i < RUNS - 1; i++) up.push(await step(app, "ArrowUp"));
      return `↓ new: ${stats(down)}; ↑ seen: ${stats(up)}`;
    },
  },
  {
    name: "outside commit",
    // A commit made in a terminal: from git's answer to the new row on screen, with the
    // repository's watch noticing it (no Reload).
    async run(ctx) {
      const repo = path.join(ctx.dir, "history-100k");
      git(ctx.dir, "clone", "-q", "--local", path.join(ctx.perf, "history-100k"), repo);
      const app = await ctx.start(repo);
      await app.js((page) => page.sleep(2000));
      const times = [];
      for (let i = 0; i < RUNS; i++) {
        git(repo, "commit", "-q", "--allow-empty", "-m", `Outside ${i}`);
        const t = Date.now();
        await app.js((page, i) => page.until(() => page.words(page.all("[aria-label=Commits] [role=option]")[0] ?? document.body).startsWith(`Outside ${i}`), "the new commit", 30_000), i);
        times.push(Date.now() - t);
        await app.js((page) => page.sleep(500));
      }
      return `commit to row: ${stats(times)}`;
    },
  },
  {
    name: "reload",
    // Reload (F5) when nothing changed, and the 100k history read again.
    async run(ctx) {
      const app = await ctx.start(path.join(ctx.perf, "history-100k-clone"));
      await app.js((page) => page.sleep(1500));
      const times = [];
      for (let i = 0; i < RUNS; i++) {
        await app.js(async (page) => {
          for (const toast of page.all("[role=status]")) toast.dataset.seen = "1";
          window.__keyAt = null;
          window.addEventListener("keydown", () => (window.__keyAt ??= performance.now()), { capture: true, once: true });
        });
        await app.keys("F5");
        times.push(
          await app.js(async (page) => {
            await page.until(() => page.all("[role=status]:not([data-seen])", "Reloaded")[0], "Reloaded", 30_000);
            return Math.round(performance.now() - window.__keyAt);
          }),
        );
        await app.js((page) => page.sleep(300));
      }
      return `F5 to «Reloaded»: ${stats(times)}`;
    },
  },
];
