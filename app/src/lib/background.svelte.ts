import { emit, listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { backgroundType } from "./background";
import { MAX_PIXELS, pictureSize, Urls, inTurn } from "./pictures";
import { prefs } from "./prefs.svelte";
import spring from "../assets/backgrounds/full/spring.webp";
import summer from "../assets/backgrounds/full/summer.webp";
import autumn from "../assets/backgrounds/full/autumn.webp";
import winter from "../assets/backgrounds/full/winter.webp";

const presets = {
  spring: { url: spring, position: "50% 60%" },
  summer: { url: summer, position: "50% 58%" },
  autumn: { url: autumn, position: "50% 55%" },
  winter: { url: winter, position: "50% 60%" },
};

export const background = $state({ url: "", position: "50% 50%", loading: false, problem: "" });
let started = false;
let revision = $state(0);

type Status = { selection: string; loading: boolean; problem: string };
const identityOf = () => JSON.stringify([prefs.get("oxbow.background.source"), prefs.get("oxbow.background.image")]);

export function retryBackground() {
  if (started) revision += 1;
  else void emit("background-retry").catch(() => {});
}

/** Settings observes the renderer's real decode result without owning another full bitmap. */
export function followBackgroundStatus() {
  const ready = listen<Status>("background-status", ({ payload }) => {
    if (payload.selection !== identityOf()) return;
    background.loading = payload.loading;
    background.problem = payload.problem;
  });
  $effect(() => {
    identityOf();
    background.problem = "";
    void ready.then(() => emit("background-status-request")).catch(() => {});
  });
  $effect(() => () => { void ready.then((stop) => stop()).catch(() => {}); });
}

/** Only rendering windows own decoded images; a displayed Blob URL lives until replacement. */
export function followBackground() {
  if (started) return;
  started = true;
  let shownVersion = "";
  const publish = () => {
    void emit("background-status", { selection: identityOf(), loading: background.loading, problem: background.problem }).catch(() => {});
  };
  void listen("background-status-request", publish).catch(() => {});
  void listen("background-retry", () => { revision += 1; }).catch(() => {});
  window.addEventListener("focus", async () => {
    if (prefs.get("oxbow.background.source") !== "custom" || background.loading) return;
    const identity = identityOf();
    try {
      const version = await api.backgroundImageVersion();
      if (identity === identityOf() && version !== shownVersion) revision += 1;
    } catch {
      if (identity === identityOf()) revision += 1;
    }
  });
  $effect.root(() => {
    let shownSelection = "";
    let shownUrls: Urls | null = null;
    const release = () => { shownUrls?.close(); shownUrls = null; };
    $effect(() => { publish(); });
    const selection = $derived(JSON.stringify([
      prefs.get("oxbow.background.source"),
      prefs.get("oxbow.background.source") === "custom" ? prefs.get("oxbow.background.image") : "",
      prefs.get("oxbow.background.source") === "custom" && (prefs.pending("oxbow.background.source") || prefs.pending("oxbow.background.image")),
    ]));
    $effect(() => {
      const [source, file, pending] = JSON.parse(selection) as ["none" | "custom" | keyof typeof presets, string, boolean];
      revision;
      let gone = false;
      const urls = new Urls();
      const identity = JSON.stringify([source, file]);
      if (identity !== shownSelection) { background.url = ""; shownVersion = ""; release(); }
      shownSelection = identity;
      background.problem = "";
      background.loading = source !== "none";
      if (source === "none" || pending) return;
      const preset = source !== "custom" ? presets[source] : null;
      background.position = preset?.position ?? "50% 50%";
      let version = "";
      inTurn(async () => {
        let url = preset?.url;
        if (!url) {
          if (!file) throw new Error("Choose a background image in Settings › Appearance.");
          version = await api.backgroundImageVersion();
          if (gone) return null;
          const data = await api.backgroundImage() as Uint8Array<ArrayBuffer>;
          if (gone) return null;
          const type = backgroundType(data, pictureSize(data), MAX_PIXELS);
          url = urls.make(new Blob([data], { type }));
        }
        const image = new Image();
        image.src = url;
        await image.decode();
        return url;
      }, () => gone).then((url) => {
        if (!gone) {
          release();
          shownUrls = urls;
          shownVersion = version;
          background.url = url ?? "";
          background.loading = false;
        }
      }, (err) => {
        urls.close();
        if (!gone) {
          release();
          shownVersion = "";
          background.url = "";
          background.loading = false;
          background.problem = `Couldn’t load the background image. ${String(err)}`;
        }
      });
      return () => { gone = true; if (shownUrls !== urls) urls.close(); };
    });
    $effect(() => () => release());
  });
}
