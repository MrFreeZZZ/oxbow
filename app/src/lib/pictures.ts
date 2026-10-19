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

/** Width and height a picture's header gives, without decoding it: PNG, GIF, JPEG, WebP and
 *  BMP. Null for other kinds or a header it can't read. */
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
