import { Decoration, EditorView, WidgetType } from "@codemirror/view";
import { RangeSetBuilder, StateField } from "@codemirror/state";
import { isExcalidrawSrc } from "./excalidraw.js";

/**
 * Legacy ```excalidraw path``` fences.
 * @param {import('@codemirror/state').Text} doc
 * @returns {{ from: number, to: number, path: string }[]}
 */
export function findExcalidrawBlocks(doc) {
  /** @type {{ from: number, to: number, path: string }[]} */
  const blocks = [];
  for (let i = 1; i <= doc.lines; i++) {
    const line = doc.line(i);
    if (!/^```excalidraw\b/.test(line.text)) continue;

    const start = line.from;
    /** @type {string[]} */
    const pathLines = [];
    let closed = false;
    for (let j = i + 1; j <= doc.lines; j++) {
      const l = doc.line(j);
      if (/^```\s*$/.test(l.text)) {
        blocks.push({
          from: start,
          to: l.to,
          path: pathLines.join("\n").trim(),
        });
        i = j;
        closed = true;
        break;
      }
      pathLines.push(l.text);
    }
    if (!closed) break;
  }
  return blocks;
}

/**
 * Image-like `![alt](path.excalidraw)` references (optional trailing `{…}` ignored for hit).
 * @param {import('@codemirror/state').Text} doc
 * @returns {{ from: number, to: number, path: string }[]}
 */
export function findExcalidrawImages(doc) {
  /** @type {{ from: number, to: number, path: string }[]} */
  const hits = [];
  const text = doc.toString();
  const re = /!\[([^\]]*)\]\(([^)\s]+)\)/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const path = m[2];
    if (!isExcalidrawSrc(path)) continue;
    hits.push({
      from: m.index,
      to: m.index + m[0].length,
      path,
    });
  }
  return hits;
}

/**
 * @param {import('@codemirror/state').Text} doc
 * @param {number} pos
 * @returns {{ from: number, to: number, path: string } | null}
 */
export function findExcalidrawAt(doc, pos) {
  for (const hit of findExcalidrawImages(doc)) {
    if (pos >= hit.from && pos <= hit.to) return hit;
  }
  for (const hit of findExcalidrawBlocks(doc)) {
    if (pos >= hit.from && pos <= hit.to) return hit;
  }
  return null;
}

class ExcalidrawWidget extends WidgetType {
  /**
   * @param {string} path
   * @param {(path: string) => void} onOpen
   */
  constructor(path, onOpen) {
    super();
    this.path = path;
    this.onOpen = onOpen;
  }

  /** @param {ExcalidrawWidget} other */
  eq(other) {
    return other.path === this.path;
  }

  toDOM() {
    const el = document.createElement("button");
    el.type = "button";
    el.className = "cm-excalidraw-widget";
    const label = this.path
      ? this.path.split("/").pop() || this.path
      : "(未指定路径)";
    el.textContent = `Excalidraw · ${label}`;
    el.title = this.path
      ? `⌘/Ctrl + 点击编辑 ${this.path}`
      : "未指定 .excalidraw 路径";
    el.addEventListener("mousedown", (e) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      e.preventDefault();
      e.stopPropagation();
      if (this.path) this.onOpen(this.path);
    });
    return el;
  }

  ignoreEvent() {
    return false;
  }
}

/**
 * @param {import('@codemirror/state').EditorState} state
 * @param {(path: string) => void} onOpen
 */
function buildDecorations(state, onOpen) {
  /** @type {{ from: number, to: number, deco: import('@codemirror/view').Decoration }[]} */
  const items = [];
  for (const block of findExcalidrawBlocks(state.doc)) {
    items.push({
      from: block.from,
      to: block.to,
      deco: Decoration.replace({
        widget: new ExcalidrawWidget(block.path, onOpen),
        block: true,
      }),
    });
  }
  for (const hit of findExcalidrawImages(state.doc)) {
    items.push({
      from: hit.from,
      to: hit.to,
      deco: Decoration.mark({ class: "cm-excalidraw-ref" }),
    });
  }
  items.sort((a, b) => a.from - b.from || a.to - b.to);
  const builder = new RangeSetBuilder();
  for (const item of items) builder.add(item.from, item.to, item.deco);
  return builder.finish();
}

/**
 * Fold legacy fences; mark image-like refs; open via ⌘/Ctrl+click.
 * @param {(path: string) => void} onOpen
 */
export function excalidrawFoldExtension(onOpen) {
  const field = StateField.define({
    create(state) {
      return buildDecorations(state, onOpen);
    },
    update(deco, tr) {
      if (tr.docChanged) return buildDecorations(tr.state, onOpen);
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  const clickHandler = EditorView.domEventHandlers({
    click(event, view) {
      if (!(event.metaKey || event.ctrlKey)) return false;
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos == null) return false;
      const hit = findExcalidrawAt(view.state.doc, pos);
      if (!hit?.path) return false;
      event.preventDefault();
      onOpen(hit.path);
      return true;
    },
  });

  return [field, clickHandler];
}
