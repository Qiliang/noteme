<script setup>
import { onBeforeUnmount, onMounted, ref } from "vue";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import "@excalidraw/excalidraw/index.css";

const props = defineProps({
  modelValue: { type: String, default: "" },
});

const emit = defineEmits(["update:modelValue"]);

const host = ref(null);
/** @type {import('react-dom/client').Root | null} */
let root = null;
/** @type {import('@excalidraw/excalidraw/types/types').ExcalidrawImperativeAPI | null} */
let api = null;
/** @type {((elements: unknown, appState: unknown, files: unknown, type: string) => string) | null} */
let serializeAsJSON = null;
/** @type {string} */
let latestJson = "";
/** Ignore library bootstrap onChange noise. */
let ignoreUntil = 0;

function parseInitialData(raw) {
  const text = (raw ?? "").trim();
  if (!text) return null;
  try {
    const data = JSON.parse(text);
    if (!data || typeof data !== "object") return null;
    return {
      elements: data.elements ?? [],
      appState: data.appState ?? {},
      files: data.files ?? {},
    };
  } catch {
    return null;
  }
}

/** Latest scene JSON (prefers live API so close/save never misses strokes). */
function serialize() {
  if (api && serializeAsJSON) {
    try {
      latestJson = serializeAsJSON(
        api.getSceneElements(),
        api.getAppState(),
        api.getFiles(),
        "local",
      );
    } catch {
      /* keep latestJson */
    }
  }
  return latestJson || props.modelValue || "";
}

onMounted(async () => {
  if (!host.value) return;

  const mod = await import("@excalidraw/excalidraw");
  serializeAsJSON = mod.serializeAsJSON;
  latestJson = props.modelValue ?? "";
  ignoreUntil = Date.now() + 400;

  root = createRoot(host.value);
  root.render(
    createElement(mod.Excalidraw, {
      // Parent remounts this component via :key when the file changes.
      initialData: parseInitialData(props.modelValue),
      langCode: "zh-CN",
      excalidrawAPI: (next) => {
        api = next;
      },
      UIOptions: {
        canvasActions: {
          loadScene: false,
          saveToActiveFile: false,
          toggleTheme: true,
        },
      },
      onChange(elements, appState, files) {
        if (!serializeAsJSON) return;
        const json = serializeAsJSON(elements, appState, files, "local");
        latestJson = json;
        if (Date.now() < ignoreUntil) return;
        emit("update:modelValue", json);
      },
    }),
  );
});

onBeforeUnmount(() => {
  // Flush one last time before unmount so parents that read serialize() are current.
  try {
    serialize();
  } catch {
    /* ignore */
  }
  api = null;
  if (root) {
    root.unmount();
    root = null;
  }
});

defineExpose({ serialize });
</script>

<template>
  <div ref="host" class="excalidraw-host" />
</template>

<style scoped>
.excalidraw-host {
  height: 100%;
  min-height: 0;
  width: 100%;
  overflow: hidden;
}

.excalidraw-host :deep(.excalidraw) {
  height: 100%;
}
</style>
