<script setup>
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useDocs } from "../../composables/useDocs.js";

const { documentRoot, files, busy, status, error, run, loadSettings } = useDocs();
const localError = ref("");
const localStatus = ref("");

async function pickRoot() {
  localError.value = "";
  localStatus.value = "";
  await run("选择文档目录…", async () => {
    const dir = await open({
      directory: true,
      multiple: false,
      title: "选择文档根目录",
    });
    if (dir == null) {
      localStatus.value = "已取消";
      return;
    }
    const path = typeof dir === "string" ? dir : String(dir);
    const current = await invoke("settings_get");
    const s = await invoke("settings_set", {
      settings: {
        ...current,
        documentRoot: path,
      },
    });
    documentRoot.value = s?.documentRoot || path;
    // Do not scan the tree here — listing is lazy per directory in the workspace.
    files.value = [];
    localStatus.value = "已设置文档根目录";
  });
}

onMounted(async () => {
  try {
    await loadSettings();
  } catch (e) {
    localError.value = typeof e === "string" ? e : e?.message || String(e);
  }
});
</script>

<template>
  <div class="doc-root">
    <header class="panel-head">
      <div>
        <h2 class="title">文档目录</h2>
        <p class="hint">笔记与附件将直接读写此操作系统目录。选择后不会扫描全部文件。</p>
      </div>
      <div class="actions">
        <button type="button" :disabled="busy" @click="pickRoot">选择目录</button>
      </div>
    </header>

    <p v-if="localError || error" class="banner err">{{ localError || error }}</p>
    <p v-else-if="localStatus || status" class="banner ok">{{ localStatus || status }}</p>

    <div class="card">
      <div class="label">当前文档根目录</div>
      <p class="path" :title="documentRoot">
        {{ documentRoot || "尚未配置 — 请点击「选择目录」" }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.doc-root {
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
  max-width: 720px;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
}

.title {
  margin: 0;
  font-family: var(--serif);
  font-size: 1.35rem;
  font-weight: 700;
}

.hint {
  margin: 0.35rem 0 0;
  font-size: 0.9rem;
  color: color-mix(in srgb, var(--ink) 65%, transparent);
}

.actions {
  display: flex;
  gap: 0.4rem;
  flex-shrink: 0;
}

.banner {
  margin: 0;
  padding: 0.55rem 0.75rem;
  border-radius: 6px;
  font-size: 0.9rem;
}

.banner.err {
  background: color-mix(in srgb, #b33 12%, transparent);
  border: 1px solid color-mix(in srgb, #b33 35%, transparent);
}

.banner.ok {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
}

.card {
  padding: 0.9rem 1rem;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: color-mix(in srgb, var(--panel) 80%, transparent);
}

.label {
  font-size: 0.8rem;
  letter-spacing: 0.02em;
  color: color-mix(in srgb, var(--ink) 55%, transparent);
  margin-bottom: 0.35rem;
}

.path {
  margin: 0;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.92rem;
  word-break: break-all;
}
</style>
