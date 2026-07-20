<script setup>
import { ref } from "vue";
import DocumentRoot from "./DocumentRoot.vue";
import FileManager from "./FileManager.vue";
import GitSync from "./GitSync.vue";
import PreviewOptions from "./PreviewOptions.vue";
import CryptoLabels from "./CryptoLabels.vue";

const emit = defineEmits(["close"]);

/** @type {import('vue').Ref<'root' | 'files' | 'git' | 'preview' | 'crypto'>} */
const section = ref("root");

const nav = [
  { id: "root", label: "文档目录" },
  { id: "files", label: "文件管理" },
  { id: "git", label: "Git 同步" },
  { id: "preview", label: "预览选项" },
  { id: "crypto", label: "密码标签" },
];
</script>

<template>
  <div class="settings">
    <header class="top">
      <div class="brand">
        <span class="mark">设置</span>
      </div>
      <div class="actions">
        <button type="button" @click="emit('close')">返回</button>
      </div>
    </header>

    <div class="body">
      <nav class="nav" aria-label="设置导航">
        <button
          v-for="item in nav"
          :key="item.id"
          type="button"
          class="nav-item"
          :class="{ active: section === item.id }"
          @click="section = item.id"
        >
          {{ item.label }}
        </button>
      </nav>

      <main class="content">
        <DocumentRoot v-if="section === 'root'" />
        <FileManager v-else-if="section === 'files'" />
        <GitSync v-else-if="section === 'git'" />
        <PreviewOptions v-else-if="section === 'preview'" />
        <CryptoLabels v-else-if="section === 'crypto'" />
      </main>
    </div>
  </div>
</template>

<style scoped>
.settings {
  height: 100%;
  display: flex;
  flex-direction: column;
  background:
    radial-gradient(1200px 500px at 10% -10%, #e7f2ee 0%, transparent 55%),
    linear-gradient(180deg, #f7f2e8 0%, var(--bg) 100%);
  color: var(--ink);
  font-family: var(--sans);
}

.top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.7rem 1.1rem;
  border-bottom: 1px solid var(--line);
  background: color-mix(in srgb, var(--panel) 88%, transparent);
  backdrop-filter: blur(8px);
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 0.75rem;
}

.mark {
  font-family: var(--serif);
  font-weight: 700;
  letter-spacing: -0.02em;
  font-size: 1.25rem;
}

.actions {
  display: flex;
  gap: 0.4rem;
}

.body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(160px, 18%) 1fr;
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.85rem 0.65rem;
  border-right: 1px solid var(--line);
  background: color-mix(in srgb, var(--panel) 70%, transparent);
}

.nav-item {
  width: 100%;
  text-align: left;
  border: 1px solid transparent;
  background: transparent;
  border-radius: 6px;
  padding: 0.55rem 0.7rem;
  font-size: 0.92rem;
}

.nav-item:hover {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}

.nav-item.active {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  border-color: color-mix(in srgb, var(--accent) 30%, transparent);
  font-weight: 600;
}

.content {
  min-height: 0;
  overflow: auto;
  padding: 1rem 1.1rem 1.25rem;
}

@media (max-width: 720px) {
  .body {
    grid-template-columns: 1fr;
    grid-template-rows: auto 1fr;
  }

  .nav {
    flex-direction: row;
    border-right: none;
    border-bottom: 1px solid var(--line);
    overflow-x: auto;
  }

  .nav-item {
    width: auto;
    white-space: nowrap;
  }
}
</style>
