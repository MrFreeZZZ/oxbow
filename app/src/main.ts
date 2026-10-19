import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import ConsoleWindow from "./lib/ConsoleWindow.svelte";
import SettingsWindow from "./lib/SettingsWindow.svelte";
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

// The Settings window loads the same page with #settings, the Console with #console.
const windowKind = location.hash === "#settings" ? SettingsWindow : location.hash === "#console" ? ConsoleWindow : App;
export default mount(windowKind, { target: document.getElementById("app")! });
// What a word of a git command means, over every window's sheets.
mount(WordPopover, { target: document.body });
