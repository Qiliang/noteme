<script setup>
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { EditorView, keymap, placeholder as cmPlaceholder } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
import { savePastedImage } from "../lib/markdown.js";
import { excalidrawFoldExtension } from "../lib/excalidrawFold.js";

const props = defineProps({
  modelValue: { type: String, default: "" },
  notePath: { type: String, default: "" },
});

const emit = defineEmits([
  "update:modelValue",
  "image-saved",
  "status",
  "error",
  "open-excalidraw",
]);

const host = ref(null);
/** @type {import('vue').Ref<EditorView | null>} */
const view = ref(null);
let applyingExternal = false;

function emitDoc(viewInstance) {
  if (applyingExternal) return;
  emit("update:modelValue", viewInstance.state.doc.toString());
}

function handlePaste(event, viewInstance) {
  const items = event.clipboardData?.items;
  if (!items || !props.notePath) return false;

  for (const item of items) {
    if (!item.type.startsWith("image/")) continue;
    const blob = item.getAsFile();
    if (!blob) continue;

    // Must return a boolean synchronously: a Promise is truthy and would
    // block CodeMirror's default text paste for every clipboard event.
    event.preventDefault();
    void (async () => {
      try {
        emit("status", "保存图片…");
        const { markdown: md, path } = await savePastedImage(props.notePath, blob);
        const { from, to } = viewInstance.state.selection.main;
        const insert = `\n${md}\n`;
        viewInstance.dispatch({
          changes: { from, to, insert },
          selection: { anchor: from + insert.length },
        });
        emit("image-saved", path);
        emit("status", "图片已插入");
      } catch (e) {
        emit("error", typeof e === "string" ? e : e?.message || String(e));
      }
    })();
    return true;
  }
  return false;
}

function createState(doc) {
  return EditorState.create({
    doc,
    extensions: [
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      markdown(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      cmPlaceholder("开始写 Markdown…"),
      EditorView.lineWrapping,
      excalidrawFoldExtension((path) => {
        emit("open-excalidraw", path);
      }),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) emitDoc(u.view);
      }),
      EditorView.domEventHandlers({
        paste(event, v) {
          return handlePaste(event, v);
        },
      }),
      EditorView.theme({
        "&": {
          height: "100%",
          fontSize: "15px",
        },
        ".cm-scroller": {
          fontFamily:
            '"IBM Plex Mono", "SF Mono", "Menlo", "Cascadia Code", monospace',
          lineHeight: "1.55",
        },
        ".cm-content": {
          padding: "1rem 1.1rem",
          caretColor: "#0f6b5c",
        },
        ".cm-gutters": {
          background: "transparent",
          border: "none",
        },
        "&.cm-focused": {
          outline: "none",
        },
        ".cm-excalidraw-widget": {
          display: "block",
          width: "100%",
          boxSizing: "border-box",
          margin: "0.35rem 0",
          padding: "0.55rem 0.75rem",
          border: "1px solid #c5ddd7",
          borderRadius: "6px",
          background: "linear-gradient(180deg, #f3faf8 0%, #e8f4f0 100%)",
          color: "#0f6b5c",
          fontFamily:
            '"IBM Plex Sans", "SF Pro Text", "Segoe UI", sans-serif',
          fontSize: "13px",
          fontWeight: "600",
          textAlign: "left",
          cursor: "pointer",
        },
        ".cm-excalidraw-widget:hover": {
          borderColor: "#0f6b5c",
          background: "linear-gradient(180deg, #e8f4f0 0%, #dceee8 100%)",
        },
        ".cm-excalidraw-ref": {
          textDecoration: "underline",
          textDecorationStyle: "dotted",
          textDecorationColor: "#0f6b5c",
          textUnderlineOffset: "3px",
          cursor: "pointer",
        },
      }),
    ],
  });
}

onMounted(() => {
  if (!host.value) return;
  view.value = new EditorView({
    state: createState(props.modelValue),
    parent: host.value,
  });
});

watch(
  () => props.modelValue,
  (next) => {
    const v = view.value;
    if (!v) return;
    const cur = v.state.doc.toString();
    if (cur === next) return;
    applyingExternal = true;
    v.dispatch({
      changes: { from: 0, to: cur.length, insert: next ?? "" },
    });
    applyingExternal = false;
  },
);

onBeforeUnmount(() => {
  view.value?.destroy();
  view.value = null;
});

function focus() {
  view.value?.focus();
}

/**
 * @param {string} text
 * @returns {boolean} false when the editor view is not ready
 */
function insertAtCursor(text) {
  const v = view.value;
  if (!v) return false;
  const { from, to } = v.state.selection.main;
  const insert = text.startsWith("\n") ? text : `\n${text}`;
  const withTrailing = insert.endsWith("\n") ? insert : `${insert}\n`;
  v.dispatch({
    changes: { from, to, insert: withTrailing },
    selection: { anchor: from + withTrailing.length },
  });
  v.focus();
  return true;
}

/** @returns {{ from: number, to: number, text: string } | null} */
function getSelection() {
  const v = view.value;
  if (!v) return null;
  const { from, to } = v.state.selection.main;
  return { from, to, text: v.state.doc.sliceString(from, to) };
}

/**
 * Replace current selection (or insert at cursor if empty).
 * @param {string} text
 * @returns {boolean}
 */
function replaceSelection(text) {
  const v = view.value;
  if (!v) return false;
  const { from, to } = v.state.selection.main;
  const insert = text ?? "";
  v.dispatch({
    changes: { from, to, insert },
    selection: { anchor: from + insert.length },
  });
  v.focus();
  return true;
}

defineExpose({ focus, insertAtCursor, getSelection, replaceSelection });
</script>

<template>
  <div ref="host" class="cm-host" />
</template>

<style scoped>
.cm-host {
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.cm-host :deep(.cm-editor) {
  height: 100%;
  background: transparent;
}
</style>
