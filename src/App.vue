<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import Workspace from "./components/Workspace.vue";
import Settings from "./components/settings/Settings.vue";

/** @type {import('vue').Ref<'workspace' | 'settings'>} */
const view = ref("workspace");

function toggleSettings() {
  view.value = view.value === "settings" ? "workspace" : "settings";
}

function onKeydown(e) {
  if ((e.metaKey || e.ctrlKey) && e.key === ",") {
    e.preventDefault();
    toggleSettings();
  }
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div class="app">
    <Workspace v-if="view === 'workspace'" @open-settings="view = 'settings'" />
    <Settings v-else @close="view = 'workspace'" />
  </div>
</template>

<style>
:root {
  --bg: #f3efe6;
  --panel: #fffdf8;
  --ink: #1c1914;
  --muted: #6b6458;
  --line: #d9d0c0;
  --accent: #0f6b5c;
  --accent-ink: #f4fffb;
  --danger-bg: #fde8e4;
  --danger: #9b2c1f;
  --ok-bg: #e5f4ea;
  --ok: #1f6b3a;
  --mono: "IBM Plex Mono", "SF Mono", "Menlo", monospace;
  --sans: "Source Sans 3", "IBM Plex Sans", "Segoe UI", sans-serif;
  --serif: "Source Serif 4", "Iowan Old Style", "Palatino Linotype", serif;
}

html,
body,
#app {
  margin: 0;
  height: 100%;
  overflow: hidden;
  overscroll-behavior: none;
}

body {
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

.app {
  height: 100%;
  overflow: hidden;
  overscroll-behavior: none;
}

.app button {
  border: 1px solid var(--line);
  background: var(--panel);
  color: var(--ink);
  border-radius: 6px;
  padding: 0.35rem 0.7rem;
  font: inherit;
  font-size: 0.88rem;
  cursor: pointer;
}

.app button:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
}

.app button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.app button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-ink);
}

.app button.danger {
  background: var(--danger);
  border-color: var(--danger);
  color: #fff;
}
</style>
