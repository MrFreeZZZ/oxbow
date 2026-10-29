// The acceptance scenarios: each starts the real app on fixtures of its own and checks what
// docs/acceptance.md says, with real git underneath. `run(ctx)` throws on a failure and may give
// back a note (a time measured, which way the app went) for the line printed.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { git, read, rebaseInConflict, repo, submodulesOneFails, withWorktree, write } from "./fixtures.mjs";

/** Waits until `test()` gives something truthy, and gives it back. */
async function eventually(test, what, ms = 10_000) {
  const end = Date.now() + ms;
  for (;;) {
    const value = await test();
    if (value) return value;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** Waits until the window shows `text` and gives the milliseconds it took. */
const shows = (app, text, ms = 10_000) => app.js((page, text, ms) => page.timed(() => {}, () => page.words(document.body).includes(text), ms), text, ms);

/** The words of the screen, for assertions on what it says. */
const screen = (app) => app.js(async (page) => page.words(document.body));

/** Opens the Conflicts screen if needed and gives the conflicted files' rows. */
const conflictFiles = (app, n) =>
  app.js(
    async (page, n) => {
      if (!page.all(".conflicts .file").length) await page.click("Resolve Conflicts");
      await page.until(() => page.all(".conflicts .file").length >= n, "the conflicted files");
      await page.sleep(300);
      return page.all(".conflicts .file").map(page.words);
    },
    n,
  );

const openConflict = (app, name) =>
  app.js(async (page, name) => {
    page.all(".conflicts .file", name)[0].click();
    await page.until(() => page.words(document.querySelector(".filebar .fname") ?? document.body).includes(name), name);
    await page.paint();
  }, name);

/** Shows the changed file `name` in Changes. */
const openChanged = (app, name) =>
  app.js(async (page, name) => {
    await page.click("Changes");
    (await page.until(() => page.all("button.name", name)[0], name)).click();
    await page.until(() => page.button("Stage Hunk"), "its diff");
  }, name);

export const SCENARIOS = [
  {
    name: "commit diff",
    // Open → pick a commit → read its diff; two quick steps show the second, not the first.
    async run(ctx) {
      const app = await ctx.start(repo(`${ctx.dir}/r`, 5));
      const first = await app.js(async (page) => {
        const row = page.all("[aria-label=Commits] [role=option]", "Commit 4")[0];
        return page.timed(() => row.click(), () => document.body.innerText.includes("file 4 1"));
      });
      await app.keys("ArrowDown", "ArrowDown");
      await shows(app, "file 2 1");
      const after = await app.js(async (page) => {
        await page.sleep(500);
        return { stale: document.body.innerText.includes("file 3 1"), selected: page.all("[aria-selected=true]").map(page.words) };
      });
      assert.equal(after.stale, false, "the diff of the commit stepped over is shown");
      assert.match(after.selected.join(), /^Commit 2/);
      // Reloaded: the same commit stays selected.
      await app.keys("F5");
      await app.js((page) => page.sleep(1000));
      const reloaded = await app.js(async (page) => page.all("[aria-selected=true]").map(page.words));
      assert.match(reloaded.join(), /^Commit 2/, "Reload keeps the selection");
      assert.match(await screen(app), /file 2 1/);
      return `first diff in ${first} ms`;
    },
  },
  {
    name: "staging after an outside edit",
    // Pick lines → stage → the file changes outside → stage again; the index has exactly that.
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 1);
      write(r, "src/f1.txt", read(r, "src/f1.txt").replace("file 1 3\nfile 1 4", "X3\nX4").replace("file 1 15", "X15"));
      const app = await ctx.start(r);
      const pick = async (...texts) => {
        for (const text of texts)
          await app.tap(
            (page, text) => [...document.querySelectorAll(".line")].find((l) => l.querySelector(".text")?.textContent.trim() === text && l.querySelector(".pick"))?.querySelector(".pick"),
            text,
          );
      };
      const index = () => git(r, "show", ":src/f1.txt");
      await openChanged(app, "f1.txt");
      await pick("file 1 3", "X3");
      await app.js((page) => page.click("Stage 2 Lines"));
      await eventually(() => /^X3$/m.test(index()), "X3 staged");
      assert.match(index(), /^file 1 4$/m, "only the picked lines are staged");
      // Changed outside: X4 becomes Y4, and the diff shows it without asking.
      write(r, "src/f1.txt", read(r, "src/f1.txt").replace("X4", "Y4"));
      const seen = await shows(app, "Y4");
      await pick("file 1 4", "Y4");
      await app.js((page) => page.click("Stage 2 Lines"));
      await eventually(() => /^X3\nY4$/m.test(index()), "Y4 staged after X3");
      assert.match(index(), /^file 1 15$/m);
      // Picked, then changed outside before Stage: what goes in is never what wasn't shown.
      await pick("file 1 15", "X15");
      write(r, "src/f1.txt", read(r, "src/f1.txt").replace("X15", "Z15"));
      const clicked = await app.js(async (page) => {
        const stage = page.button("Stage 2 Lines");
        stage?.click();
        await page.sleep(1500);
        return !!stage;
      });
      assert.doesNotMatch(index(), /Z15/, "a line never shown was staged");
      const last = !clicked ? "the picks went when the file changed" : /X15/.test(index()) ? "the lines shown were staged" : "Stage was refused";
      return `an outside change shows in ${seen} ms; picked, then changed outside: ${last}`;
    },
  },
  {
    name: "conflicts kept over a restart",
    // A rebase stopped in conflicts: a side picked, lines typed, another screen, the app quit
    // and started again; then Continue to the next step, which starts with no old decisions.
    async run(ctx) {
      const r = rebaseInConflict(`${ctx.dir}/r`);
      let app = await ctx.start(r);
      assert.deepEqual(await conflictFiles(app, 2), ["! a.txt 1 conflict left", "! b.txt Conflicted"]);
      await openConflict(app, "a.txt");
      await app.js(async (page) => page.all(".result .open .pick", "feature")[0].click());
      await openConflict(app, "b.txt");
      await app.js(async (page) => page.all(".result .open .pick", "Edit")[0].click());
      await app.tap(() => document.querySelector(".typed .cm-content"));
      await app.keys("Ctrl+a", "typed by hand");
      await shows(app, "Draft saved");
      await app.js((page) => page.click("History"));
      app = await ctx.start(r);
      const again = await conflictFiles(app, 2);
      assert.deepEqual(again, ["! a.txt Ready to mark resolved", "! b.txt Ready to mark resolved"], "decisions kept over a restart");
      await openConflict(app, "b.txt");
      assert.match(await screen(app), /typed by you .*typed by hand/, "typed lines kept over a restart");
      await app.js((page) => page.act("Mark Resolved"));
      await openConflict(app, "a.txt");
      await app.js((page) => page.act("Mark Resolved"));
      await app.js((page) => page.until(() => page.button("Continue Rebase") && !page.button("Continue Rebase").disabled, "Continue"));
      await app.js((page) => page.act("Continue Rebase"));
      assert.deepEqual(await conflictFiles(app, 1), ["! b.txt 1 conflict left"]);
      const step2 = await screen(app);
      assert.match(step2, /Commit 2 of 2/);
      assert.match(step2, /Pick a side, keep both, or edit/, "the next step starts undecided");
      assert.equal(read(r, "b.txt").includes("typed by hand"), true, "step 1's typed lines are in its commit");
    },
  },
  {
    name: "skip and abort",
    // Skip leaves the first commit out and stops on the next; Abort puts the branch and files
    // back as they were before the rebase.
    async run(ctx) {
      const r = rebaseInConflict(`${ctx.dir}/r`);
      const before = git(r, "rev-parse", "ORIG_HEAD");
      const app = await ctx.start(r);
      await conflictFiles(app, 2);
      const skip = await app.js((page) => page.act("Skip Commit"));
      await shows(app, "Commit 2 of 2");
      const abort = await app.js((page) => page.act("Abort Rebase"));
      assert.match(abort, /git rebase --abort/, "the confirmation shows the git command");
      await app.js((page) => page.until(() => !page.button("Abort Rebase"), "the rebase to end", 30_000));
      assert.equal(git(r, "rev-parse", "HEAD"), before, "HEAD is back where it was");
      assert.equal(git(r, "branch", "--show-current"), "feature");
      assert.equal(git(r, "status", "--porcelain"), "");
      return skip ? "Skip asked first" : "Skip went to Activity";
    },
  },
  {
    name: "two windows",
    // The main folder and a worktree in two windows: a commit in one is in the other's history,
    // and a palette opened in one leaves the other alone.
    async run(ctx) {
      const { main, side } = withWorktree(`${ctx.dir}/r`);
      const app = await ctx.start(main);
      const first = await app.window();
      await app.js(async (page) => {
        const row = await page.until(() => page.all(".sidebar *, nav *").find((el) => el.ondblclick !== undefined && page.words(el).startsWith("r-side")) ?? page.all("[role=button], button, li, div", "r-side").at(-1), "the worktree");
        row.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      });
      const handles = await eventually(async () => (await app.windows()).length === 2 && (await app.windows()), "a second window");
      const other = (await handles).find((h) => h !== first);
      await app.switchTo(other);
      await app.js((page) => page.until(() => page.all("[aria-label=Commits] [role=option]").length, "the worktree's history"));
      write(side, "side.txt", "made in the worktree\n");
      git(side, "add", "-A");
      git(side, "commit", "-q", "-m", "Commit in the worktree");
      const here = await shows(app, "Commit in the worktree");
      await app.switchTo(first);
      const there = await shows(app, "Commit in the worktree", 15_000);
      await app.switchTo(other);
      await app.keys("Ctrl+Shift+p");
      await app.js((page) => page.until(() => page.all("[aria-label='Command Palette']").length, "the palette"));
      await app.switchTo(first);
      const palette = await app.js(async (page) => page.all("[aria-label='Command Palette']").length);
      assert.equal(palette, 0, "the palette opened in the other window too");
      return `a commit in the worktree shows in its window in ${here} ms, in the main one in ${there} ms`;
    },
  },
  {
    name: "missing folder",
    // The open worktree's folder is deleted: the window says so and opens the main folder.
    async run(ctx) {
      const { main, side } = withWorktree(`${ctx.dir}/r`);
      const app = await ctx.start(side);
      fs.rmSync(side, { recursive: true, force: true });
      await shows(app, "isn’t there any more", 15_000);
      const said = await screen(app);
      assert.match(said, /The branch side and its commits are safe in r/);
      await app.js(async (page) => page.button("Open Main Folder")?.click());
      await app.js((page) => page.until(() => page.all("[aria-label=Commits] [role=option]").length && page.words(document.body).includes("Commit 2"), "the main folder's history"));
      return git(main, "worktree", "list").includes("prunable") ? "git marks it prunable" : "";
    },
  },
  {
    name: "submodules, one fails",
    // Update All with one submodule whose remote is gone: the others are updated, the one is
    // named, and Try Again updates only it once its remote is back.
    async run(ctx) {
      const { dir, gone, source } = submodulesOneFails(`${ctx.dir}/r`);
      const at = (name) => git(path.join(dir, "libs", name), "rev-parse", "HEAD");
      const recorded = (name) => git(dir, "rev-parse", `HEAD:libs/${name}`);
      const app = await ctx.start(dir);
      await app.js(async (page) => {
        await page.click("Submodule actions");
        await page.click("Update All");
      });
      const said = await app.js(async (page) => page.words(await page.until(() => page.all("[role=alertdialog]", "Updated 2 of 3")[0], "the partial result", 60_000)));
      assert.match(said, /lib-c couldn’t be updated/);
      assert.equal(at("lib-a"), recorded("lib-a"));
      assert.equal(at("lib-b"), recorded("lib-b"));
      assert.notEqual(at("lib-c"), recorded("lib-c"));
      fs.renameSync(gone, source);
      await app.js((page) => page.click("Try lib-c Again"));
      await eventually(() => at("lib-c") === recorded("lib-c"), "lib-c updated", 30_000);
    },
  },
  {
    name: "undo a discard",
    // Discard a changed file, then Undo: it comes back as it was.
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 2);
      write(r, "src/f1.txt", "changed by hand\n");
      const app = await ctx.start(r);
      await openChanged(app, "f1.txt");
      const asked = await app.js((page) => page.act("Discard"));
      await eventually(() => read(r, "src/f1.txt") !== "changed by hand\n", "the file discarded");
      await app.js((page) => page.act("Undo"));
      await eventually(() => read(r, "src/f1.txt") === "changed by hand\n", "the file back");
      return asked ? "Discard asked first" : "Discard went to Activity";
    },
  },
  {
    name: "a status git can't read",
    // The index is broken: the window says the changes are unknown, never «No changes».
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 2);
      write(r, "src/f1.txt", "changed\n");
      const app = await ctx.start(r);
      await shows(app, "1 changed");
      fs.writeFileSync(path.join(r, ".git", "index"), "not an index");
      await app.keys("F5");
      await shows(app, "Changes unknown");
      assert.doesNotMatch(await screen(app), /No changes/);
      fs.rmSync(path.join(r, ".git", "index"));
      git(r, "reset", "-q");
      await app.keys("F5");
      await shows(app, "1 changed");
    },
  },
  {
    name: "a setting that can't be saved",
    // settings.json can't be written (a folder is where its new copy goes): Settings says so
    // and the switch goes back.
    async run(ctx) {
      const app = await ctx.start(repo(`${ctx.dir}/r`, 1));
      fs.mkdirSync(path.join(ctx.config, "settings.json.new"), { recursive: true });
      const main = await app.window();
      await app.js((page) => page.click("Settings"));
      const handles = await eventually(async () => (await app.windows()).length === 2 && (await app.windows()), "the Settings window");
      await app.switchTo(handles.find((h) => h !== main));
      const before = await app.js(async (page) => (await page.until(() => page.all("[role=switch]")[0], "a switch")).getAttribute("aria-checked"));
      await app.tap((page) => page.all("[role=switch]")[0]);
      await shows(app, "Couldn’t save");
      const after = await app.js(async (page) => page.all("[role=switch]")[0].getAttribute("aria-checked"));
      assert.equal(after, before, "the switch shows a setting that wasn't saved");
    },
  },
  {
    name: "an Undo that can't be kept",
    // .git/oxbow is a file, so the state before can't be saved: Discard says so before it runs.
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 2);
      write(r, "src/f1.txt", "changed\n");
      fs.writeFileSync(path.join(r, ".git", "oxbow"), "");
      const app = await ctx.start(r);
      await openChanged(app, "f1.txt");
      const said = await app.js(async (page) => {
        await page.click("Discard");
        await page.answer("Discard");
        const sheet = await page.until(() => page.all("[role=alertdialog]", "Undo isn’t ready")[0], "a warning that Undo can't be kept", 5000);
        return page.words(sheet);
      });
      assert.equal(read(r, "src/f1.txt"), "changed\n", "discarded with no way back and no warning");
      assert.match(said, /undo/i, "the warning doesn't name Undo");
      await app.keys("Escape");
      assert.equal(read(r, "src/f1.txt"), "changed\n");
      return `«${said.slice(0, 200)}»`;
    },
  },
  {
    name: "keys and modals",
    // The palette opens on its keys, Escape closes it and gives the focus back; a confirmation
    // keeps Tab inside; ⌘P-like keys work on a Russian layout too.
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 3);
      const app = await ctx.start(r);
      await app.tap((page) => page.all("[aria-label=Commits] [role=option]", "Commit 2")[0]);
      const before = await app.js(async (page) => page.focused());
      await app.keys("Ctrl+Shift+p");
      await app.js((page) => page.until(() => page.all("[aria-label='Command Palette']").length && document.activeElement?.tagName === "INPUT", "the palette, focused"));
      await app.keys("Escape");
      const after = await app.js(async (page) => {
        await page.until(() => !page.all("[aria-label='Command Palette']").length, "the palette to close");
        await page.paint();
        await page.paint();
        return page.focused();
      });
      assert.equal(after, before, `the focus didn't come back: ${before} → ${after}`);
      // A confirmation keeps Tab inside it, and Escape cancels it.
      write(r, "src/f1.txt", "changed\n");
      await shows(app, "1 changed");
      await openChanged(app, "f1.txt");
      await app.js(async (page) => {
        await page.click("Discard");
        await page.until(() => page.all("[role=alertdialog]").length, "the confirmation");
      });
      for (let i = 0; i < 6; i++) await app.keys("Tab");
      const inside = await app.js(async (page) => !!document.activeElement?.closest("[role=alertdialog]"));
      assert.equal(inside, true, "Tab left the confirmation");
      await app.keys("Escape");
      await app.js((page) => page.until(() => !page.all("[role=alertdialog]").length, "the confirmation to close"));
      assert.equal(read(r, "src/f1.txt"), "changed\n");
      // Russian layout: Ctrl+Shift and the key where P is (З) open the palette.
      if (!hasXdotool()) return "no xdotool: the Russian layout is left to the Mac checklist";
      execFileSync("setxkbmap", ["-layout", "us,ru", "-option", "grp:alt_shift_toggle"]);
      try {
        execFileSync("xdotool", ["key", "--clearmodifiers", "ctrl+shift+Cyrillic_ze"]);
        await app.js((page) => page.until(() => page.all("[aria-label='Command Palette']").length, "the palette on a Russian layout", 3000));
      } finally {
        execFileSync("setxkbmap", ["-layout", "us"]);
      }
    },
  },
  {
    name: "a file of a commit",
    // Revert This File from a commit's file menu changes only that file on disk, and Undo takes
    // it back; ↑ in the empty summary brings the last message, and Sign-off signs the commit.
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 2);
      git(r, "config", "user.name", "Fixture");
      git(r, "config", "user.email", "fixture@example.com");
      write(r, "src/f1.txt", read(r, "src/f1.txt").replace("file 1 5", "file 1 five"));
      write(r, "src/f2.txt", read(r, "src/f2.txt").replace("file 2 5", "file 2 five"));
      git(r, "commit", "-q", "-am", "Spell five");
      const app = await ctx.start(r);
      await app.js(async (page) => {
        page.all("[aria-label=Commits] [role=option]", "Spell five")[0].click();
        const file = await page.until(() => page.all(".files .file", "f1.txt")[0], "the commit's file");
        const box = file.getBoundingClientRect();
        file.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.left + 20, clientY: box.top + 8 }));
        await page.paint();
        await page.act("Revert This File", "Revert File");
      });
      await eventually(() => !read(r, "src/f1.txt").includes("five"), "the file reverted");
      assert.match(read(r, "src/f2.txt"), /file 2 five/, "the commit's other file changed too");
      assert.equal(git(r, "log", "-1", "--format=%s"), "Spell five");
      assert.equal(git(r, "diff", "--cached", "--name-only"), "", "the revert was staged");
      await app.js((page) => page.act("Undo"));
      await eventually(() => read(r, "src/f1.txt").includes("five"), "the file back");

      write(r, "src/f1.txt", "changed\n");
      await shows(app, "1 changed");
      await app.js(async (page) => {
        await page.click("Changes");
        (await page.until(() => document.querySelector("input[aria-label='Commit summary']"), "the summary box")).focus();
      });
      await app.keys("ArrowUp");
      const recalled = await app.js(async (page) => page.until(() => document.querySelector("input[aria-label='Commit summary']").value, "a message"));
      assert.equal(recalled, "Spell five");
      await app.keys("ArrowDown");
      assert.equal(await app.js(async (page) => document.querySelector("input[aria-label='Commit summary']").value), "", "↓ went back to the empty box");
      await app.keys("Sign the change");
      await app.js(async (page) => {
        await page.click("Sign-off");
        await page.click("Stage 1 File");
        await page.until(() => page.words(document.body).includes("1 file staged"), "the file staged");
        document.querySelector("input[aria-label='Commit summary']").focus();
      });
      await app.keys("Ctrl+Enter");
      await eventually(() => git(r, "log", "-1", "--format=%s") === "Sign the change", "the commit");
      assert.equal(git(r, "log", "-1", "--format=%b").trim(), "Signed-off-by: Fixture <fixture@example.com>");
    },
  },
  {
    name: "a changelist",
    // A file's menu puts it in a new changelist, a hunk's chip moves one change of another file
    // there, and Commit… commits the list alone: the staged file stays staged, the rest unstaged.
    async run(ctx) {
      const r = repo(`${ctx.dir}/r`, 3);
      git(r, "config", "user.name", "Fixture");
      git(r, "config", "user.email", "fixture@example.com");
      write(r, "src/f1.txt", read(r, "src/f1.txt").replace("file 1 3", "file 1 three"));
      write(r, "src/f2.txt", read(r, "src/f2.txt").replace("file 2 2", "file 2 two").replace("file 2 18", "file 2 eighteen"));
      write(r, "src/f3.txt", read(r, "src/f3.txt").replace("file 3 1", "file 3 one"));
      git(r, "add", "src/f3.txt");
      const app = await ctx.start(r);
      await app.js(async (page) => {
        await page.click("Changes");
        const row = await page.until(() => page.all('.file[data-group="unstaged"]', "f1.txt")[0], "f1.txt");
        const box = row.getBoundingClientRect();
        row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.left + 60, clientY: box.top + 8 }));
        await page.click("Move to Changelist");
        (await page.until(() => page.all(".submenu [role=menuitem]", "New Changelist")[0], "New Changelist…")).click();
        const name = await page.until(() => document.querySelector("[role=alertdialog] input.text"), "the name box");
        name.value = "Fix";
        name.dispatchEvent(new Event("input", { bubbles: true }));
        await page.paint();
        await page.answer("Create");
        await page.until(() => page.all(".gtitle", "Fix").length, "the list");
        // f2.txt's second change goes to Fix too.
        page.all('.file[data-group="unstaged"] button.name', "f2.txt")[0].click();
        const chips = await page.until(() => (page.all(".hunk .chip").length === 2 ? page.all(".hunk .chip") : null), "two hunks");
        chips[1].click();
        (await page.until(() => page.all("[role=menu] [role=menuitem]").find((b) => page.words(b) === "Fix"), "Fix in the menu")).click();
        await page.until(() => page.all(".note.partial", "1 of 2").length === 2, "f2.txt in both lists");
        await page.click("Commit…");
        await page.until(() => document.querySelector("input[aria-label='Commit summary']").value === "Fix", "the list's name as the summary");
      });
      assert.ok(fs.existsSync(path.join(r, ".git/oxbow/changelists.json")), "the lists are kept in .git/oxbow");
      await app.js(async (page) => {
        const go = page.all(".commit button.go")[0];
        go.click();
        const end = performance.now() + 2000;
        while (performance.now() < end) {
          const sheet = page.all("[role=alertdialog]")[0];
          const yes = sheet && [...sheet.querySelectorAll("button")].find((b) => page.words(b) === "Commit");
          if (yes) return yes.click();
          await page.sleep(20);
        }
      });
      await eventually(() => git(r, "log", "-1", "--format=%s") === "Fix", "the list's commit");
      assert.equal(git(r, "show", "--name-only", "--format=", "HEAD"), "src/f1.txt\nsrc/f2.txt");
      assert.match(git(r, "show", "HEAD", "--", "src/f2.txt"), /file 2 eighteen/);
      assert.doesNotMatch(git(r, "show", "HEAD", "--", "src/f2.txt"), /file 2 two/);
      assert.equal(git(r, "diff", "--cached", "--name-only"), "src/f3.txt", "the staged file stays staged");
      assert.equal(git(r, "diff", "--name-only"), "src/f2.txt");
      assert.match(git(r, "diff"), /file 2 two/);
      await eventually(() => !fs.existsSync(path.join(r, ".git/oxbow/changelists.json")), "the list gone with its last change");
    },
  },
];

function hasXdotool() {
  try {
    execFileSync("which", ["xdotool", "setxkbmap"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}
