<script setup>
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { EditorView, keymap, placeholder as cmPlaceholder } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
import { savePastedImage } from "../lib/markdown.js";

const props = defineProps({
  modelValue: { type: String, default: "" },
  notePath: { type: String, default: "" },
});

const emit = defineEmits(["update:modelValue", "image-saved", "status", "error"]);

const host = ref(null);
/** @type {import('vue').Ref<EditorView | null>} */
const view = ref(null);
let applyingExternal = false;

function emitDoc(viewInstance) {
  if (applyingExternal) return;
  emit("update:modelValue", viewInstance.state.doc.toString());
}

async function handlePaste(event, viewInstance) {
  const items = event.clipboardData?.items;
  if (!items || !props.notePath) return false;

  for (const item of items) {
    if (!item.type.startsWith("image/")) continue;
    const blob = item.getAsFile();
    if (!blob) continue;

    event.preventDefault();
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

defineExpose({ focus });
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
