import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import WordPopover from "./lib/WordPopover.svelte";
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

// The Settings window loads the same page with #settings, the Console with #console. Only the
// main window's code is read up front: the other two load theirs.
const target = document.getElementById("app")!;
if (location.hash === "#settings") void import("./lib/SettingsWindow.svelte").then((m) => mount(m.default, { target }));
else if (location.hash === "#console") void import("./lib/ConsoleWindow.svelte").then((m) => mount(m.default, { target }));
else mount(App, { target });
// What a word of a git command means, over every window's sheets.
mount(WordPopover, { target: document.body });
