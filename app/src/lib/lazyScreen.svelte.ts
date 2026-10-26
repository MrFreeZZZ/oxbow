// Screens and sheets most windows never show: their code loads the first time one is shown, so the
// window starts with less to read. Loading is a file from the app itself, so it is there by the
// next frame; until then nothing is drawn in the screen's place.

/** One screen's component, `import()`ed when first asked for. */
export class Lazy<C> {
  #load: () => Promise<{ default: C }>;
  #loading: Promise<unknown> | null = null;
  #loaded = $state<C | null>(null);

  constructor(load: () => Promise<{ default: C }>) {
    this.#load = load;
  }

  /** The screen, or null while its code is still loading; asking starts the load. */
  get component(): C | null {
    this.#loading ??= this.#load().then(
      (module) => (this.#loaded = module.default),
      (error) => {
        // Asked again, it tries again.
        this.#loading = null;
        console.error(error);
      },
    );
    return this.#loaded;
  }
}
