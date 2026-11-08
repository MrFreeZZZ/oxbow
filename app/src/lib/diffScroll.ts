import { lane } from "./format";
import type { Hunk } from "./types";

/** Keep both sides of a large replacement in the preview, even before Lazy draws its rows. */
export function changePreview(hunk: Hunk): string {
  let removed = 0;
  let added = 0;
  const lines = hunk.lines.filter(line => line.kind === "removed" ? removed++ < 3 : line.kind === "added" ? added++ < 3 : false);
  return JSON.stringify((lines.length ? lines : hunk.lines.slice(0, 6)).map(line => ({
    kind: line.kind,
    text: line.text.length > 180 ? `${line.text.slice(0, 180)}…` : line.text,
  })));
}

/** One navigation rail for the entire scrolling diff, including lazy blocks in every file. */
export function diffScroll(root: HTMLElement, color: number) {
  root.classList.add("diff-scroll", "thin-scroll");
  const anchor = document.createElement("div");
  anchor.className = "diff-rail-anchor";
  const rail = document.createElement("nav");
  rail.className = "diff-rail";
  rail.style.setProperty("--diff-rail-color", lane(color));
  rail.setAttribute("aria-label", "Changes in this diff");
  anchor.append(rail);
  root.prepend(anchor);

  type Mark = { element: HTMLElement; button: HTMLButtonElement; top: number; bottom: number; inset: number };
  let marks: Mark[] = [];
  let frame = 0;
  let dirty = true;
  let focused = 0;
  let hovered: Mark | null = null;
  let escaped = false;
  let dismissTimer: ReturnType<typeof setTimeout> | undefined;
  const preview = document.createElement("div");
  preview.className = "diff-preview";
  preview.id = `diff-preview-${crypto.randomUUID()}`;
  preview.setAttribute("role", "tooltip");
  preview.hidden = true;
  anchor.append(preview);

  function hidePreview() {
    clearTimeout(dismissTimer);
    hovered?.button.removeAttribute("aria-describedby");
    hovered = null;
    preview.hidden = true;
    rail.classList.remove("previewing");
    for (const mark of marks) mark.button.style.removeProperty("--mark-width");
  }

  function dismissSoon() {
    clearTimeout(dismissTimer);
    dismissTimer = setTimeout(hidePreview, 160);
  }

  function showPreview(mark: Mark) {
    clearTimeout(dismissTimer);
    hovered?.button.removeAttribute("aria-describedby");
    hovered = mark;
    mark.button.setAttribute("aria-describedby", preview.id);
    const path = mark.element.closest<HTMLElement>("section.file")?.dataset.path ?? "Change";
    const title = document.createElement("div");
    title.className = "diff-preview-title";
    title.textContent = path;
    const range = document.createElement("div");
    range.className = "diff-preview-range";
    range.textContent = (mark.element.dataset.diffChange ?? "").slice(path.length).replace(/^ · /, "");
    const code = document.createElement("div");
    code.className = "diff-preview-code";
    const lines: { kind: string; text: string }[] = JSON.parse(mark.element.dataset.diffPreview ?? "[]");
    for (const line of lines) {
      const row = document.createElement("div");
      row.className = `diff-preview-line ${line.kind}`;
      // Diff text is repository content, never markup.
      row.textContent = line.text || " ";
      code.append(row);
    }
    preview.replaceChildren(title, range, code);
    preview.hidden = false;
    rail.classList.add("previewing");
    const index = marks.indexOf(mark);
    marks.forEach((item, i) => item.button.style.setProperty("--mark-width", `${Math.max(8, 32 - Math.abs(i - index) * 6)}px`));
    const box = mark.button.getBoundingClientRect();
    const width = preview.offsetWidth;
    const height = preview.offsetHeight;
    const left = box.right + 12;
    preview.style.left = `${Math.max(8, Math.min(left, window.innerWidth - width - 8))}px`;
    preview.style.top = `${Math.max(8, Math.min(box.top + box.height / 2 - height / 2, window.innerHeight - height - 8))}px`;
  }

  function schedule(measure = false) {
    dirty ||= measure;
    if (!frame) frame = requestAnimationFrame(update);
  }

  function jump(mark: Mark) {
    // Read the position afresh: mounting a lazy block can correct its estimated height.
    const top = mark.element.getBoundingClientRect().top - root.getBoundingClientRect().top + root.scrollTop;
    root.scrollTo({ top: Math.max(0, top - mark.inset), behavior: "instant" });
  }

  const resized = new ResizeObserver(() => schedule(true));
  const observed = new Set<Element>();
  resized.observe(root);
  function measure() {
    const elements = [...root.querySelectorAll<HTMLElement>("[data-diff-change]")];
    const old = new Map(marks.map(mark => [mark.element, mark]));
    const view = root.getBoundingClientRect();
    const nextObserved = new Set<Element>([...elements, ...root.querySelectorAll("section.file")]);
    for (const element of observed) if (!nextObserved.has(element)) resized.unobserve(element);
    for (const element of nextObserved) if (!observed.has(element)) resized.observe(element);
    observed.clear();
    for (const element of nextObserved) observed.add(element);
    marks = elements.map(element => {
      let mark = old.get(element);
      if (!mark) {
        const button = document.createElement("button");
        button.type = "button";
        mark = { element, button, top: 0, bottom: 0, inset: 0 };
        const target = mark;
        button.addEventListener("click", () => jump(target));
        button.addEventListener("pointerenter", () => { if (!escaped) showPreview(target); });
        button.addEventListener("pointerleave", dismissSoon);
        button.addEventListener("focus", () => {
          escaped = false;
          focused = marks.indexOf(target);
          for (const item of marks) item.button.tabIndex = item === target ? 0 : -1;
          showPreview(target);
        });
        button.addEventListener("blur", dismissSoon);
      }
      const label = element.dataset.diffChange ?? "Change";
      mark.button.setAttribute("aria-label", label);
      const box = element.getBoundingClientRect();
      mark.top = box.top - view.top + root.scrollTop;
      mark.bottom = mark.top + box.height;
      mark.inset = element.closest("section.file")?.querySelector("header")?.getBoundingClientRect().height ?? 0;
      return mark;
    });
    // Preserve focused buttons across lazy mounting and geometry updates.
    const wanted = new Set(marks.map(mark => mark.button));
    for (const child of [...rail.children]) if (!wanted.has(child as HTMLButtonElement)) child.remove();
    marks.forEach((mark, i) => {
      if (rail.children[i] !== mark.button) rail.insertBefore(mark.button, rail.children[i] ?? null);
    });
    focused = Math.min(focused, Math.max(0, marks.length - 1));
    marks.forEach((mark, i) => (mark.button.tabIndex = i === focused ? 0 : -1));
    if (hovered && !marks.includes(hovered)) hidePreview();
    anchor.hidden = marks.length === 0;
    rail.style.maxHeight = `${Math.max(0, root.clientHeight - 16)}px`;
    rail.style.top = `${Math.max(8, (root.clientHeight - rail.offsetHeight) / 2)}px`;
  }

  function update() {
    frame = 0;
    if (dirty) {
      dirty = false;
      measure();
    }
    let first: Mark | undefined;
    for (const mark of marks) {
      const visible = mark.bottom > root.scrollTop + mark.inset && mark.top < root.scrollTop + root.clientHeight;
      mark.button.classList.toggle("visible", visible);
      if (visible && !first) first = mark;
    }
    // A long rail can scroll independently; keep the visible change in reach unless in use.
    if (first && !rail.matches(":hover, :focus-within")) {
      const y = first.button.offsetTop;
      if (y < rail.scrollTop || y + first.button.offsetHeight > rail.scrollTop + rail.clientHeight) rail.scrollTop = y - rail.clientHeight / 2;
    }
  }

  function onKey(event: KeyboardEvent) {
    const index = marks.findIndex(mark => mark.button === event.target);
    if (index < 0) return;
    let next: number;
    switch (event.key) {
      case "ArrowDown": next = Math.min(index + 1, marks.length - 1); break;
      case "ArrowUp": next = Math.max(index - 1, 0); break;
      case "Home": next = 0; break;
      case "End": next = marks.length - 1; break;
      default: return;
    }
    event.preventDefault();
    marks[next].button.focus({ preventScroll: true });
    rail.scrollTop = marks[next].button.offsetTop - rail.clientHeight / 2;
    jump(marks[next]);
  }

  const changed = new MutationObserver(records => {
    if (records.some(record => !anchor.contains(record.target))) schedule(true);
  });
  changed.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-diff-change", "data-diff-preview"] });
  const onScroll = () => { hidePreview(); schedule(); };
  const onResize = () => { hidePreview(); schedule(true); };
  const onEscape = (event: KeyboardEvent) => {
    if (hovered && event.key === "Escape") {
      // Shrinking the rail can put another mark under a stationary pointer.
      escaped = true;
      hidePreview();
      event.preventDefault();
      event.stopPropagation();
    }
  };
  rail.addEventListener("pointermove", event => {
    if (!escaped || (!event.movementX && !event.movementY)) return;
    escaped = false;
    const button = (event.target as Element).closest("button");
    const mark = marks.find(item => item.button === button);
    if (mark) showPreview(mark);
  });
  preview.addEventListener("pointerenter", () => clearTimeout(dismissTimer));
  preview.addEventListener("pointerleave", dismissSoon);
  rail.addEventListener("scroll", hidePreview, { passive: true });
  window.addEventListener("resize", onResize);
  document.addEventListener("keydown", onEscape, true);
  root.addEventListener("scroll", onScroll, { passive: true });
  rail.addEventListener("keydown", onKey);
  schedule(true);
  return {
    update(color: number) {
      rail.style.setProperty("--diff-rail-color", lane(color));
    },
    destroy() {
      hidePreview();
      cancelAnimationFrame(frame);
      changed.disconnect();
      resized.disconnect();
      root.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onResize);
      document.removeEventListener("keydown", onEscape, true);
      rail.removeEventListener("keydown", onKey);
      anchor.remove();
      root.classList.remove("diff-scroll", "thin-scroll");
    },
  };
}
