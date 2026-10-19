// Pictures in a diff are read and decoded only as they come into view, a few at a time, and a
// picture of too many pixels waits to be asked for: two sides of 12,000 × 12,000 take a
// gigabyte to draw.

/** Pictures read and decoded at once; the others wait their turn. */
const AT_ONCE = 2;
/** Pixels of one side drawn without asking: about 7,000 × 7,000. */
export const MAX_PIXELS = 50_000_000;

let running = 0;
const waiting: (() => void)[] = [];

/** Runs `work` once fewer than [`AT_ONCE`] others run; null, without running it, when `gone`
 *  says its answer is no longer wanted by then. */
export async function inTurn<T>(work: () => Promise<T>, gone: () => boolean): Promise<T | null> {
  if (running >= AT_ONCE) await new Promise<void>((go) => waiting.push(go));
  running++;
  try {
    return gone() ? null : await work();
  } finally {
    running--;
    waiting.shift()?.();
  }
}

/** Width and height a picture's header gives, without decoding it: PNG, GIF, JPEG, WebP, BMP,
 *  AVIF/HEIC and ICO. Null for other kinds (SVG is drawn at the size it is shown) or a header it
 *  can't read. */
export function pictureSize(data: Uint8Array): { width: number; height: number } | null {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const has = (n: number) => data.length >= n;
  const ascii = (at: number, text: string) => [...text].every((c, i) => data[at + i] === c.charCodeAt(0));
  if (has(24) && data[0] === 0x89 && ascii(1, "PNG") && ascii(12, "IHDR")) {
    return { width: view.getUint32(16), height: view.getUint32(20) };
  }
  if (has(10) && ascii(0, "GIF8")) return { width: view.getUint16(6, true), height: view.getUint16(8, true) };
  if (has(26) && ascii(0, "BM")) return { width: Math.abs(view.getInt32(18, true)), height: Math.abs(view.getInt32(22, true)) };
  if (has(32) && ascii(0, "RIFF") && ascii(8, "WEBP")) {
    if (ascii(12, "VP8X")) return { width: 1 + (view.getUint32(24, true) & 0xffffff), height: 1 + (view.getUint32(27, true) & 0xffffff) };
    if (ascii(12, "VP8L")) {
      const bits = view.getUint32(21, true);
      return { width: 1 + (bits & 0x3fff), height: 1 + ((bits >> 14) & 0x3fff) };
    }
    if (ascii(12, "VP8 ")) return { width: view.getUint16(26, true) & 0x3fff, height: view.getUint16(28, true) & 0x3fff };
  }
  if (has(12) && ascii(4, "ftyp")) {
    // AVIF and HEIC: each image item has an `ispe` box (size, "ispe", version and flags, width,
    // height); the largest is the picture, the others its tiles or thumbnails.
    let best: { width: number; height: number } | null = null;
    const end = Math.min(data.length, 1 << 20) - 16;
    for (let at = 8; at <= end; at++) {
      if (data[at] !== 0x69 || !ascii(at, "ispe")) continue;
      const width = view.getUint32(at + 8);
      const height = view.getUint32(at + 12);
      if (!best || width * height > best.width * best.height) best = { width, height };
    }
    return best;
  }
  if (has(22) && view.getUint16(0, true) === 0 && view.getUint16(2, true) === 1) {
    // ICO: a directory of images of at most 256 × 256 (0 stands for 256).
    const count = view.getUint16(4, true);
    let best: { width: number; height: number } | null = null;
    for (let i = 0; i < count && 6 + 16 * i + 2 <= data.length; i++) {
      const width = data[6 + 16 * i] || 256;
      const height = data[7 + 16 * i] || 256;
      if (!best || width * height > best.width * best.height) best = { width, height };
    }
    return best;
  }
  if (has(4) && data[0] === 0xff && data[1] === 0xd8) {
    // JPEG: the frame header (SOF0–SOF15 but DHT, JPG and DAC) holds the size.
    let at = 2;
    while (at + 9 < data.length) {
      if (data[at] !== 0xff) return null;
      const marker = data[at + 1];
      if (marker === 0xff) {
        at++;
        continue;
      }
      const length = view.getUint16(at + 2);
      if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
        return { width: view.getUint16(at + 7), height: view.getUint16(at + 5) };
      }
      at += 2 + length;
    }
  }
  return null;
}

/** The Blob URLs of one reading of a picture's two sides. Each is owned from the moment it is
 *  made, so none is left behind when the other side fails or the view closes first. */
export class Urls {
  private list: string[] = [];
  private closed = false;

  make(blob: Blob): string {
    const url = URL.createObjectURL(blob);
    if (this.closed) URL.revokeObjectURL(url);
    else this.list.push(url);
    return url;
  }

  /** Lets go of every URL made, and of any made from now on. */
  close() {
    this.closed = true;
    for (const url of this.list) URL.revokeObjectURL(url);
    this.list = [];
  }
}

export type Picture = { url: string; width: number; height: number; size: number };
export type Size = { width: number; height: number };

/** One side of a picture: drawn from a URL of `urls`, or its size when it has too many pixels to
 *  draw without asking (`all` draws it anyway). */
export async function loadSide<S>(
  source: S | null,
  type: string,
  all: boolean,
  urls: Urls,
  read: (source: S) => Promise<Uint8Array>,
  decode: (url: string) => Promise<Size>,
): Promise<Picture | Size | null> {
  if (!source) return null;
  const data = (await read(source)) as Uint8Array<ArrayBuffer>;
  const size = pictureSize(data);
  if (!all && size && size.width * size.height > MAX_PIXELS) return size;
  const url = urls.make(new Blob([data], { type }));
  const drawn = await decode(url);
  return { url, ...drawn, size: data.byteLength };
}

/** Both sides of a picture's change; when one fails, the URL of the other is let go too. */
export async function loadPair<S>(
  sources: [S | null, S | null],
  type: string,
  all: boolean,
  urls: Urls,
  read: (source: S) => Promise<Uint8Array>,
  decode: (url: string) => Promise<Size>,
): Promise<(Picture | Size | null)[]> {
  try {
    return await Promise.all(sources.map((s) => loadSide(s, type, all, urls, read, decode)));
  } catch (err) {
    urls.close();
    throw err;
  }
}
