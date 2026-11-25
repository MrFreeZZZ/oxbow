// Runs against fixture repositories in the production WebView. Gate replies at the IPC boundary
// so History still runs against Git and the real App and RepoWindow handle the delayed response.
export async function exerciseRepositoryTabQueue(page, paths) {
  const { a, b, c, d, missing, extra } = paths;
  const tab = (folder) => page.all(".repo-tab-view.active [data-tab-id]").find(el => el.title === folder);
  const label = (folder) => folder.split("/").at(-1);
  const selected = (folder) => tab(folder)?.getAttribute("aria-selected") === "true";
  const panel = (folder) => document.querySelector(".repo-tab-view.active[role=tabpanel]")?.getAttribute("aria-label") === label(folder);
  const welcome = () => page.all(".repo-tab-view.active .welcome-body").length > 0;
  const shown = (folder) => page.until(() => selected(folder) && panel(folder), `${label(folder)} selected and visible`);
  const key = (shiftKey = false) => window.dispatchEvent(new KeyboardEvent("keydown", {
    key: "Tab", ctrlKey: true, shiftKey, bubbles: true, cancelable: true,
  }));
  const openStarts = [];
  const opened = new Map();
  const gates = new Map();
  const passedHistory = new Set();
  const callbacks = window.__TAURI_INTERNALS__.callbacks;
  const fetchIpc = window.fetch;
  const stringify = JSON.stringify;
  const intercepted = new Map();
  let refuseClose = false;
  let closeAttempts = 0;
  function intercept(command, payload, id) {
    if (intercepted.has(id)) return intercepted.get(id);
    const callback = callbacks.get(id);
    if (!callback) return payload;
    if (command === "close_repo_tabs") {
      closeAttempts++;
      // Send a stale active session so the real atomic close command refuses the batch.
      if (refuseClose) payload = { ...payload, previous: null };
    } else if (command === "open_repo_tab") {
      const { path } = payload;
      openStarts.push(path);
      callbacks.set(id, (result) => {
        if (result.session !== undefined) opened.set(result.session, path);
        return callback(result);
      });
    } else if (command === "history") {
      const folder = opened.get(payload.repoSession);
      const gate = gates.get(folder);
      if (!gate) return payload;
      callbacks.set(id, async (result) => {
        gate.started = true;
        await gate.wait;
        passedHistory.add(folder);
        return callback(result);
      });
    } else return payload;
    intercepted.set(id, payload);
    return payload;
  }
  window.fetch = (input, init) => {
    const id = init?.headers?.get?.("Tauri-Callback");
    if (id != null && typeof init.body === "string") {
      const command = String(input).split("/").at(-1);
      const payload = JSON.parse(init.body);
      const next = intercept(command, payload, Number(id));
      if (next !== payload) init = { ...init, body: stringify(next) };
    }
    return fetchIpc.call(window, input, init);
  };
  // Production CSP can send IPC through WebKit's native message handler instead of fetch.
  JSON.stringify = (value, ...args) => {
    if (value?.options?.customProtocolIpcBlocked && Number.isInteger(value.callback)) {
      const payload = intercept(value.cmd, value.payload, value.callback);
      if (payload !== value.payload) value = { ...value, payload };
    }
    return stringify(value, ...args);
  };
  function hold(folder) {
    const { promise: wait, resolve: release } = Promise.withResolvers();
    const gate = { wait, release, started: false };
    gates.set(folder, gate);
    return gate;
  }

  try {
    await shown(a);
    const extraRead = hold(extra);
    const bRead = hold(b);
    await page.click("Open recent repository");
    const option = await page.until(
      () => page.all('[aria-label="Open a recent repository"] [role=option]').find(el => el.querySelector(".detail")?.textContent === extra),
      "the extra repository in Open Recent",
    );
    option.click();
    await page.until(() => extraRead.started, "an ordinary App open waiting for History");
    tab(b).click();
    await page.until(() => selected(b), "restored B highlighted");
    key();
    await page.until(() => selected(c), "Ctrl+Tab advanced from B to C");
    key(true);
    await page.until(() => selected(b), "Ctrl+Shift+Tab reselected B");
    extraRead.release();
    await page.until(() => bRead.started, "B reached the window queue while App.openingPath was set");

    await page.click(`Close ${label(extra)} tab`);
    await page.until(() => !tab(extra), "background repository closed");
    await page.click(`Close ${label(a)} tab`);
    await page.until(() => !tab(a) && welcome(), "source screen closed during B's History");
    bRead.release();
    await shown(b);
    await page.until(() => openStarts.includes(c), "the second restored request was also queued");
    if (openStarts.filter(folder => folder === b).length !== 1) throw new Error("B opened twice");

    const dRead = hold(d);
    await page.click("Repository menu");
    await page.click("Oxbow Home");
    await page.until(() => selected("Oxbow Home") && welcome(), "Home visible");
    key(true);
    await page.until(() => dRead.started && selected(d) && welcome(), "Home to highlighted D");
    key();
    await shown(b);
    dRead.release();
    await page.until(() => passedHistory.has(d), "D History released");
    await page.paint();
    await shown(b);

    tab(c).click();
    await shown(c);
    refuseClose = true;
    const attempts = closeAttempts;
    await page.click(`Close ${label(c)} tab`);
    await page.until(() => closeAttempts > attempts && document.body.innerText.includes("The open repositories changed. Try again."), "close refusal visible");
    if (!selected(c) || !panel(c) || openStarts.includes(missing)) throw new Error("a refused close changed selection or opened its neighbor");

    refuseClose = false;
    await page.click(`Close ${label(c)} tab`);
    await page.until(() => !tab(c) && openStarts.includes(missing), "missing neighbor tried after close");
    await shown(d);
    await page.click(`Close ${label(d)} tab`);
    await page.until(() => !tab(d) && openStarts.filter(folder => folder === missing).length === 2, "the neighbor retried after closing D");
    await shown(b);
    await page.click(`Close ${label(b)} tab`);
    await page.until(() => !tab(b) && welcome() && !selected(missing) &&
      openStarts.filter(folder => folder === missing).length === 3 &&
      page.all(".repo-tab-view.active .welcome-body [role=alert]").length, "Welcome with the error after closing the last open repository");
  } finally {
    for (const gate of gates.values()) gate.release();
    window.fetch = fetchIpc;
    JSON.stringify = stringify;
  }
}
