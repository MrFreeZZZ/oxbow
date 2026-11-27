import { settingsRoute } from "./lib/settingsNavigation";
import { mount } from "svelte";
import "./app.css";
import RepoWindow from "./RepoWindow.svelte";
import WordPopover from "./lib/WordPopover.svelte";
import UpdateRestartGate from "./lib/UpdateRestartGate.svelte";
import WindowBackground from "./lib/WindowBackground.svelte";
import { followDisplay, followLook } from "./lib/prefs.svelte";

// Start in the system's look so nothing flashes; the Appearance setting takes over once read.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
followLook();
followDisplay();
// Selected rows go grey while the window is in the background, as in macOS lists.
const windowState = () => (document.documentElement.dataset.window = document.hasFocus() ? "active" : "inactive");
addEventListener("focus", windowState);
addEventListener("blur", windowState);
windowState();

// Auxiliary windows use a hash route and load their views on demand.
const target = document.getElementById("app")!;
if (settingsRoute(location.hash) !== null) void import("./lib/SettingsWindow.svelte").then((m) => mount(m.default, { target }));
else if (location.hash === "#about") void import("./lib/AboutWindow.svelte").then((m) => mount(m.default, { target }));
else if (location.hash === "#console") void import("./lib/ConsoleWindow.svelte").then((m) => mount(m.default, { target }));
else if (location.hash === "#command") void import("./lib/UserCommandWindow.svelte").then((m) => mount(m.default, { target }));
else if (location.hash === "#commit-summary") void import("./lib/CommitSummaryWindow.svelte").then((m) => mount(m.default, { target }));
else {
  mount(WindowBackground, { target: document.body });
  mount(RepoWindow, { target });
}
// What a word of a git command means, over every window's sheets.
if (settingsRoute(location.hash) !== null || ["#console", "#command", "#commit-summary"].includes(location.hash)) {
  mount(WordPopover, { target: document.body });
}

mount(UpdateRestartGate, { target: document.body });
