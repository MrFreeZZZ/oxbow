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

export function retryBackground() { revision += 1; }

/** Each window owns its decoded image and releases Blob URLs when its selection changes. */
export function followBackground() {
  if (started) return;
  started = true;
  window.addEventListener("focus", () => {
    if (prefs.get("oxbow.background.source") === "custom") retryBackground();
  });
  $effect.root(() => {
    let shownSelection = "";
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
      // Keep the already drawn image while checking the same file after focus or Retry.
      if (identity !== shownSelection) background.url = "";
      shownSelection = identity;
      background.problem = "";
      background.loading = source !== "none";
      if (source === "none" || pending) return;
      const preset = source !== "custom" ? presets[source] : null;
      background.position = preset?.position ?? "50% 50%";
      inTurn(async () => {
        let url = preset?.url;
        if (!url) {
          if (!file) throw new Error("Choose a background image in Settings › Appearance.");
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
        if (!gone) { background.url = url ?? ""; background.loading = false; }
      }, (err) => {
        urls.close();
        if (!gone) {
          background.url = "";
          background.loading = false;
          background.problem = `Couldn’t load the background image. ${String(err)}`;
        }
      });
      return () => { gone = true; urls.close(); };
    });
  });
}
