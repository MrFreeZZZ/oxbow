// A small WebDriver client for the app built by `tauri build`, driven through tauri-driver
// (scripts/acceptance.sh starts it, under Xvfb). No packages: Node's own fetch.

const SERVER = process.env.WEBDRIVER ?? "http://127.0.0.1:4444";

async function wd(method, path, body) {
  const res = await fetch(SERVER + path, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json();
  if (json.value && json.value.error) throw new Error(`${method} ${path}: ${json.value.error}: ${json.value.message}`);
  return json.value;
}

// The keys WebDriver sends for the names used here (W3C WebDriver, «Keyboard actions»).
const KEYS = {
  Escape: "", Tab: "", Enter: "", Backspace: "", Space: "",
  ArrowUp: "", ArrowDown: "", ArrowLeft: "", ArrowRight: "",
  Shift: "", Ctrl: "", Alt: "", Meta: "",
  F1: "", F5: "",
};

/** What every page script gets as its first argument. Runs in the page, so it is sent as source. */
function pageHelpers() {
  const visible = (el) => !!el && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";
  // As read, not as typeset: no direction marks, plain hyphens and spaces.
  const words = (el) =>
    (el.innerText ?? el.textContent ?? "")
      .replace(/[\u200e\u200f]/g, "")
      .replace(/\u2011/g, "-")
      .replace(/\s+/g, " ")
      .trim();
  const page = {
    visible,
    words,
    sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
    /** Until the next frame is painted. */
    paint: () => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0))),
    /** Waits until `test()` gives something truthy, and gives it back. */
    async until(test, what = String(test), ms = 15_000) {
      const end = performance.now() + ms;
      for (;;) {
        const value = test();
        if (value) return value;
        if (performance.now() > end) throw new Error(`timed out waiting for ${what}`);
        await new Promise((r) => setTimeout(r, 10));
      }
    },
    /** Visible elements matching `selector`, those whose text has `text` when given. */
    all(selector, text) {
      return [...document.querySelectorAll(selector)].filter((el) => visible(el) && (text === undefined || words(el).includes(text)));
    },
    /** The visible button (or menu item, or link) whose own text or label is `label`. */
    button(label) {
      const named = (el) => words(el) === label || words(el).replace(/\s+\d+$/, "") === label || el.getAttribute("aria-label") === label || el.title === label;
      return page.all("button, [role=menuitem], [role=button], a, [role=tab]").find(named);
    },
    async click(label) {
      const el = await page.until(() => page.button(label), `a button «${label}»`);
      if (el.disabled) throw new Error(`«${label}» is disabled: ${el.title}`);
      el.click();
      await page.paint();
    },
    /** Answers the confirmation on screen with its button `label`. */
    async answer(label) {
      const yes = await page.until(() => {
        const sheet = page.all("[role=alertdialog]")[0];
        return sheet && [...sheet.querySelectorAll("button")].find((b) => words(b) === label && !b.disabled);
      }, `a confirmation with «${label}»`);
      yes.click();
      await page.paint();
    },
    /** Clicks `label`, then the confirmation's `answer` if it asks («Do you really want to…»).
     *  Gives back the confirmation's words, or "" when it went straight to Activity. */
    async act(label, answer = label) {
      await page.click(label);
      const end = performance.now() + 1500;
      while (performance.now() < end) {
        const sheet = page.all("[role=alertdialog]")[0];
        const yes = sheet && [...sheet.querySelectorAll("button")].find((b) => words(b) === answer);
        if (yes && !yes.disabled) {
          const said = words(sheet);
          yes.click();
          await page.until(() => !page.all("[role=alertdialog]").length, "the confirmation to close", 30_000);
          return said;
        }
        await page.sleep(20);
      }
      return "";
    },
    /** Milliseconds from `act()` until `test()` holds and the frame is painted. */
    async timed(act, test = () => true, ms = 60_000) {
      const t = performance.now();
      await act();
      await page.until(test, String(test), ms);
      await page.paint();
      return Math.round(performance.now() - t);
    },
    focused: () => {
      const el = document.activeElement;
      return el ? `${el.tagName.toLowerCase()}${el.className ? "." + String(el.className).split(" ")[0] : ""} «${words(el).slice(0, 40)}»` : "nothing";
    },
  };
  return page;
}

/** One run of the app: `await App.start(binary)`, then `js`, `keys`, `windows`…, then `quit()`. */
export class App {
  constructor(id) {
    this.id = id;
  }

  static async start(binary) {
    const session = await wd("POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application: binary } } },
    });
    const app = new App(session.sessionId);
    await wd("POST", `/session/${app.id}/timeouts`, { script: 120_000 });
    return app;
  }

  /** Runs `fn` in the page as an async function and gives back what it returns. `fn` is sent as
   *  its source, so it can use only what the page has, `page` (below) and its arguments. */
  async js(fn, ...args) {
    const script = `const done = arguments[arguments.length - 1];
      const page = (${pageHelpers})();
      (${fn}).apply(null, [page, ...Array.from(arguments).slice(0, -1)])
        .then((value) => done({ value }), (err) => done({ error: String(err) + "\\n" + ((err && err.stack) || "") }));`;
    let out;
    for (let tries = 0; ; tries++) {
      try {
        out = await wd("POST", `/session/${this.id}/execute/async`, { script, args });
        break;
      } catch (err) {
        // The page was replaced while the script ran (the window went to Welcome, say): again.
        // So a script must be safe to run twice: it looks for what it clicks, and returns no
        // element, as one that went away fails the answer too.
        if (tries > 3 || !/stale element/.test(err.message)) throw err;
        await new Promise((r) => setTimeout(r, 200));
      }
    }
    if (out && out.error) throw new Error(out.error);
    return out?.value;
  }

  /** Real key presses, as the keyboard sends them: `keys("Ctrl+Shift+P")`, `keys("Escape")`,
   *  `keys("text to type")`. A chord holds its modifiers down while the last key is pressed. */
  async keys(...chords) {
    const actions = [];
    for (const chord of chords) {
      const parts = chord.length > 1 && chord.includes("+") ? chord.split("+") : [chord];
      const named = parts.map((p) => KEYS[p] ?? p);
      if (parts.length === 1 && !KEYS[chord]) {
        for (const ch of chord) actions.push({ type: "keyDown", value: ch }, { type: "keyUp", value: ch });
        continue;
      }
      for (const k of named) actions.push({ type: "keyDown", value: k });
      for (const k of named.reverse()) actions.push({ type: "keyUp", value: k });
    }
    await wd("POST", `/session/${this.id}/actions`, { actions: [{ type: "key", id: "keyboard", actions }] });
    await wd("DELETE", `/session/${this.id}/actions`);
  }

  /** A real click, with the mouse, on the element `find(page, ...args)` gives (waited for). */
  async tap(find, ...args) {
    const at = await this.js(
      `async (page, ...args) => {
        const el = await page.until(() => (${find})(page, ...args), "the element to click");
        el.scrollIntoView({ block: "nearest" });
        await page.paint();
        const box = el.getBoundingClientRect();
        return { x: Math.round(box.left + box.width / 2), y: Math.round(box.top + box.height / 2) };
      }`,
      ...args,
    );
    const actions = [
      { type: "pointerMove", duration: 0, origin: "viewport", x: at.x, y: at.y },
      { type: "pointerDown", button: 0 },
      { type: "pointerUp", button: 0 },
    ];
    await wd("POST", `/session/${this.id}/actions`, {
      actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" }, actions }],
    });
    await wd("DELETE", `/session/${this.id}/actions`);
  }

  async windows() {
    return wd("GET", `/session/${this.id}/window/handles`);
  }

  async window() {
    return wd("GET", `/session/${this.id}/window`);
  }

  async switchTo(handle) {
    await wd("POST", `/session/${this.id}/window`, { handle });
  }

  async screenshot() {
    return Buffer.from(await wd("GET", `/session/${this.id}/screenshot`), "base64");
  }

  async quit() {
    await wd("DELETE", `/session/${this.id}`).catch(() => {});
  }
}
