// The soft arrows of the design's stack strip: rounded blocks, each with a tip pointing left
// into the next one, newest on the left.

/** Polygon with rounded corners, as an SVG path. */
export function roundPoly(points: [number, number][], radii: number[]): string {
  return (
    points
      .map((p, i) => {
        const a = points[(i - 1 + points.length) % points.length];
        const b = points[(i + 1) % points.length];
        const la = Math.hypot(p[0] - a[0], p[1] - a[1]);
        const lb = Math.hypot(b[0] - p[0], b[1] - p[1]);
        const ra = Math.min(radii[i], la / 2);
        const rb = Math.min(radii[i], lb / 2);
        const s = [p[0] + ((a[0] - p[0]) * ra) / la, p[1] + ((a[1] - p[1]) * ra) / la];
        const e = [p[0] + ((b[0] - p[0]) * rb) / lb, p[1] + ((b[1] - p[1]) * rb) / lb];
        return `${i ? "L" : "M"}${s[0].toFixed(1)} ${s[1].toFixed(1)}Q${p[0]} ${p[1]} ${e[0].toFixed(1)} ${e[1].toFixed(1)}`;
      })
      .join("") + "Z"
  );
}

export const ARROW_TIP = 14;
const GAP = 4;

/** `n` arrows filling `width`, newest (index 0) on the left: each one's path, and the box its
 *  text goes in. `weights`, one per arrow in the same order, share the width out; equal without. */
export function softArrows(n: number, width: number, height: number, weights?: number[]): { path: string; left: number; width: number }[] {
  const total = width + (n - 1) * (ARROW_TIP - GAP);
  const w = weights && weights.length === n ? weights : Array<number>(n).fill(1);
  const sum = w.reduce((a, b) => a + b, 0);
  // Widths in build order: oldest first, left to right.
  const widths = Array.from({ length: n }, (_, i) => (total * w[n - 1 - i]) / sum);
  return Array.from({ length: n }, (_, display) => {
    // Built oldest first, left to right (tip on the right, notch on the left), then mirrored.
    const i = n - 1 - display;
    const cw = widths[i];
    const x0 = widths.slice(0, i).reduce((x, c) => x + c - ARROW_TIP + GAP, 0);
    const x1 = x0 + cw;
    const points: [number, number][] = [
      [x0, 0],
      [x1 - ARROW_TIP, 0],
      [x1, height / 2],
      [x1 - ARROW_TIP, height],
      [x0, height],
    ];
    const radii = [10, 10, 12, 10, 10];
    if (i > 0) {
      points.push([x0 + ARROW_TIP, height / 2]);
      radii.push(7);
    }
    const bx = x0 + (i ? ARROW_TIP : 0);
    const bw = cw - ARROW_TIP - (i ? ARROW_TIP : 0);
    return { path: roundPoly(points.map(([x, y]) => [width - x, y]), radii), left: width - bx - bw, width: bw };
  });
}
