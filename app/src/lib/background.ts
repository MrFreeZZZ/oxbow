/** Detect the supported raster formats before creating a Blob or decoding its pixels. */
export function backgroundType(data: Uint8Array, size: { width: number; height: number } | null, maxPixels: number): string {
  const starts = (at: number, bytes: number[]) => bytes.every((byte, i) => data[at + i] === byte);
  const type = starts(0, [137, 80, 78, 71, 13, 10, 26, 10]) ? "image/png"
    : starts(0, [255, 216, 255]) ? "image/jpeg"
    : starts(0, [82, 73, 70, 70]) && starts(8, [87, 69, 66, 80]) ? "image/webp" : null;
  if (!type) throw new Error("Choose a JPEG, PNG or WebP image.");
  if (!size || !size.width || !size.height) throw new Error("The background image has an unreadable size.");
  if (size.width * size.height > maxPixels) throw new Error("Choose an image with at most 50 million pixels.");
  return type;
}

/** A perceptual scale keeps the default visible and the strongest setting readable. */
export function backgroundOpacity(intensity: number, dark: boolean): number {
  return Math.sqrt(Math.max(0, Math.min(100, intensity)) / 100) * (dark ? 0.5 : 0.35);
}
