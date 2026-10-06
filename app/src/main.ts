import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import SettingsWindow from "./lib/SettingsWindow.svelte";
import { followLook } from "./lib/prefs.svelte";

// Start in the system's look so nothing flashes; the Appearance setting takes over once read.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
followLook();

// The Settings window loads the same page with #settings.
const settingsWindow = location.hash === "#settings";
export default mount(settingsWindow ? SettingsWindow : App, { target: document.getElementById("app")! });
