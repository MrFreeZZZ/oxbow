<script lang="ts">
  // Lines of code to type in, with line numbers and each line's tint: a CodeMirror editor, which
  // draws only the lines on screen, so a file of 100,000 lines types as fast as a few. It grows
  // with its text and scrolls with the page it is on.

  import { onMount, untrack } from "svelte";
  import { Compartment, EditorState, RangeSetBuilder, StateEffect } from "@codemirror/state";
  import { Decoration, EditorView, ViewPlugin, keymap, lineNumbers, type DecorationSet, type ViewUpdate } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, insertTab } from "@codemirror/commands";

  let {
    value,
    first = 1,
    tintOf = () => undefined,
    label,
    focus = false,
    onedit,
  }: {
    /** The starting text; what is typed then goes to `onedit`. */
    value: string;
    /** The number of the first line. */
    first?: number;
    /** A background for line `i`, from 0. */
    tintOf?: (i: number, line: string) => string | undefined;
    label: string;
    /** Takes the focus when it first shows, with the caret at the end. */
    focus?: boolean;
    onedit: (value: string) => void;
  } = $props();

  /** What is typed is handed on once typing pauses, and a long text after a longer pause: the
   *  screen it is on may be long too, and making the text whole again costs more than a key. */
  const LONG = 100_000;
  const pauseFor = (length: number) => (length < LONG ? 80 : 300);

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Typed and not handed on yet. */
  let unsent = false;
  const numbers = new Compartment();
  const retint = StateEffect.define<null>();

  /** Hand what is typed to `onedit` now, if it is waiting. */
  export function flush() {
    clearTimeout(timer);
    if (!unsent || !view) return;
    unsent = false;
    onedit(view.state.doc.toString());
  }

  /** Scroll line `i`, from 0, to the top of the page it is on. */
  export function showLine(i: number) {
    if (!view || i < 0 || i >= view.state.doc.lines) return;
    view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.line(i + 1).from, { y: "start", yMargin: 8 }) });
  }

  const numbered = (from: number) => lineNumbers({ formatNumber: (n) => String(from + n - 1) });

  /** The tints of the lines on screen, kept on their lines while typing moves them. */
  const tints = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = this.paint(view);
      }
      update(update: ViewUpdate) {
        if (update.viewportChanged || update.transactions.some((t) => t.effects.some((e) => e.is(retint)))) this.decorations = this.paint(update.view);
        else if (update.docChanged) this.decorations = this.decorations.map(update.changes);
      }
      paint(view: EditorView): DecorationSet {
        const out = new RangeSetBuilder<Decoration>();
        for (const { from, to } of view.visibleRanges) {
          for (let at = from; at <= to; ) {
            const line = view.state.doc.lineAt(at);
            const color = tintOf(line.number - 1, line.text);
            if (color) out.add(line.from, line.from, Decoration.line({ attributes: { style: `background: ${color}` } }));
            at = line.to + 1;
          }
        }
        return out.finish();
      }
    },
    { decorations: (plugin) => plugin.decorations },
  );

  const theme = EditorView.theme({
    "&": { backgroundColor: "transparent", color: "var(--code)", fontSize: "inherit" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "inherit", lineHeight: "inherit", overflow: "visible" },
    ".cm-content": { padding: "0", caretColor: "var(--text)", fontFamily: "inherit" },
    ".cm-line": { padding: "0 8px 0 0" },
    ".cm-cursor": { borderLeftColor: "var(--text)" },
    ".cm-gutters": { backgroundColor: "transparent", border: "none", color: "var(--text2)" },
    ".cm-lineNumbers .cm-gutterElement": { minWidth: "var(--gutter, 36px)", padding: "0 10px 0 0", boxSizing: "border-box", textAlign: "right", opacity: "0.7" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "color-mix(in srgb, var(--accent) 24%, transparent)",
    },
  });

  onMount(() => {
    const tab = parseInt(getComputedStyle(host).tabSize) || 4;
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          numbers.of(numbered(untrack(() => first))),
          history(),
          EditorState.tabSize.of(tab),
          EditorView.lineWrapping,
          EditorView.contentAttributes.of({ "aria-label": label, spellcheck: "false", autocorrect: "off", autocapitalize: "off" }),
          // Tab types a tab, as in an editor; Escape leaves the text, so the keyboard isn't stuck.
          keymap.of([{ key: "Tab", run: insertTab }, ...historyKeymap, ...defaultKeymap]),
          EditorView.domEventHandlers({
            keydown(event, view) {
              if (event.key !== "Escape") return false;
              event.stopPropagation();
              view.contentDOM.blur();
              return true;
            },
            blur: () => flush(),
          }),
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            unsent = true;
            clearTimeout(timer);
            timer = setTimeout(flush, pauseFor(update.state.doc.length));
          }),
          tints,
          theme,
        ],
      }),
    });
    if (focus) {
      view.focus();
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    }
    return () => {
      flush();
      view?.destroy();
      view = null;
    };
  });

  // Numbers follow the line the text starts at.
  $effect(() => {
    const from = first;
    untrack(() => view?.dispatch({ effects: numbers.reconfigure(numbered(from)) }));
  });

  // Tints follow whatever `tintOf` reads, such as where the markers are now: asking it about a
  // line watches that.
  $effect(() => {
    tintOf(0, "");
    untrack(() => view?.dispatch({ effects: retint.of(null) }));
  });
</script>

<div class="box selectable" bind:this={host}></div>

<style>
  .box {
    border-radius: var(--radius-m);
    overflow: hidden;
  }
  .box:focus-within {
    box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--accent) 55%, transparent);
  }
</style>
