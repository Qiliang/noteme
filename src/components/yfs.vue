<script setup>
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import MarkdownEditor from "./MarkdownEditor.vue";
import {
  isMarkdownFile,
  isTextEditable,
  renderMarkdown,
} from "../lib/markdown.js";

const storePath = ref("");
const files = ref([]);
const prefix = ref("");
const selected = ref(null);
const busy = ref(false);
const status = ref("");
const error = ref("");
const dragging = ref(false);
const fileInput = ref(null);
/** @type {import('vue').Ref<'edit' | 'preview'>} */
const mode = ref("edit");
const content = ref("");
const previewHtml = ref("");
const dirty = ref(false);
const loadToken = ref(0);

/** @type {import('vue').Ref<null | { kind: 'delete', name: string } | { kind: 'overwrite', name: string, resolve: (ok: boolean) => void } | { kind: 'new' }>} */
const dialog = ref(null);
const newName = ref("untitled.md");

function askOverwrite(name) {
  return new Promise((resolve) => {
    dialog.value = {
      kind: "overwrite",
      name,
      resolve: (ok) => {
        dialog.value = null;
        resolve(ok);
      },
    };
  });
}

function askDelete(name) {
  dialog.value = { kind: "delete", name };
}

function closeDialog() {
  if (dialog.value?.kind === "overwrite") {
    dialog.value.resolve(false);
  } else {
    dialog.value = null;
  }
}

const breadcrumbs = computed(() => {
  if (!prefix.value) return [{ label: "root", path: "" }];
  const parts = prefix.value.replace(/\/$/, "").split("/");
  const crumbs = [{ label: "root", path: "" }];
  let acc = "";
  for (const part of parts) {
    acc = acc ? `${acc}/${part}` : part;
    crumbs.push({ label: part, path: `${acc}/` });
  }
  return crumbs;
});

const folders = computed(() => {
  const set = new Set();
  const p = prefix.value;
  for (const f of files.value) {
    if (!f.name.startsWith(p)) continue;
    const rest = f.name.slice(p.length);
    const slash = rest.indexOf("/");
    if (slash > 0) set.add(rest.slice(0, slash));
  }
  return [...set].sort();
});

const visibleFiles = computed(() => {
  const p = prefix.value;
  return files.value
    .filter((f) => {
      if (!f.name.startsWith(p)) return false;
      const rest = f.name.slice(p.length);
      return rest.length > 0 && !rest.includes("/");
    })
    .sort((a, b) => a.name.localeCompare(b.name));
});

const canEdit = computed(() => {
  if (!selected.value) return false;
  return isTextEditable(selected.value.name, selected.value.fileType);
});

const isMd = computed(() => selected.value && isMarkdownFile(selected.value.name));

function formatSize(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function shortName(full) {
  if (!prefix.value) return full;
  return full.startsWith(prefix.value) ? full.slice(prefix.value.length) : full;
}

function sanitizeRelPath(rel) {
  return rel.replace(/\\/g, "/").replace(/^\/+/, "").replace(/\/+/g, "/");
}

function targetNameForFile(file) {
  const rel = sanitizeRelPath(file.webkitRelativePath || file.name);
  return `${prefix.value}${rel}`;
}

async function run(label, fn) {
  busy.value = true;
  error.value = "";
  status.value = label;
  try {
    await fn();
  } catch (e) {
    error.value = typeof e === "string" ? e : e?.message || String(e);
  } finally {
    busy.value = false;
    if (!error.value && status.value === label) status.value = "";
  }
}

async function openStore() {
  await run("打开存储…", async () => {
    storePath.value = await invoke("yfs_open");
    await refresh();
  });
}

async function refresh() {
  files.value = await invoke("yfs_list", { prefix: "" });
  if (selected.value) {
    selected.value = files.value.find((f) => f.name === selected.value.name) || null;
  }
}

async function selectFile(f) {
  if (dirty.value && selected.value) {
    await saveCurrent();
  }
  selected.value = f;
  mode.value = "edit";
  await loadSelected();
}

async function loadSelected() {
  const f = selected.value;
  const token = ++loadToken.value;
  content.value = "";
  previewHtml.value = "";
  dirty.value = false;
  if (!f || !isTextEditable(f.name, f.fileType)) return;

  await run("读取…", async () => {
    const text = await invoke("yfs_read", { name: f.name });
    if (token !== loadToken.value) return;
    content.value = text;
    dirty.value = false;
  });
}

function onContentUpdate(v) {
  content.value = v;
  dirty.value = true;
}

async function saveCurrent() {
  if (!selected.value || !canEdit.value) return;
  await run("保存…", async () => {
    await invoke("yfs_write", {
      name: selected.value.name,
      content: content.value,
    });
    dirty.value = false;
    await refresh();
    status.value = "已保存";
  });
}

async function switchMode(next) {
  if (next === mode.value) return;
  if (next === "preview") {
    if (dirty.value) await saveCurrent();
    await run("渲染预览…", async () => {
      previewHtml.value = await renderMarkdown(content.value, {
        notePath: selected.value?.name,
      });
      mode.value = "preview";
    });
  } else {
    mode.value = "edit";
  }
}

function deleteFile() {
  if (!selected.value) return;
  askDelete(selected.value.name);
}

async function confirmDelete() {
  const name = dialog.value?.kind === "delete" ? dialog.value.name : null;
  dialog.value = null;
  if (!name) return;
  await run("删除…", async () => {
    await invoke("yfs_delete", { name });
    if (selected.value?.name === name) {
      selected.value = null;
      content.value = "";
      previewHtml.value = "";
      dirty.value = false;
    }
    await refresh();
    status.value = "已删除";
  });
}

function openNewDialog() {
  newName.value = `${prefix.value}untitled.md`;
  dialog.value = { kind: "new" };
}

async function confirmNew() {
  let name = newName.value.trim().replace(/^\/+/, "");
  if (!name) return;
  if (!/\.[^/]+$/.test(name)) name = `${name}.md`;
  dialog.value = null;

  if (files.value.some((f) => f.name === name)) {
    const ok = await askOverwrite(name);
    if (!ok) return;
  }

  await run("创建…", async () => {
    const initial = `# ${name.split("/").pop().replace(/\.md$/i, "")}\n\n`;
    await invoke("yfs_write", { name, content: initial });
    await refresh();
    const f = files.value.find((x) => x.name === name);
    if (f) await selectFile(f);
    status.value = "已创建";
  });
}

async function compactStore() {
  await run("整理碎片…", async () => {
    await invoke("yfs_compact");
    status.value = "整理完成";
  });
}

async function importBrowserFiles(fileList) {
  const list = [...fileList].filter((f) => f && f.size >= 0);
  if (!list.length) return;

  const existing = new Set(files.value.map((f) => f.name));
  const plan = [];
  for (const file of list) {
    const name = targetNameForFile(file);
    if (existing.has(name) || plan.some((p) => p.name === name)) {
      const overwrite = await askOverwrite(name);
      if (!overwrite) continue;
    }
    plan.push({ file, name });
  }
  if (!plan.length) return;

  await run(`导入 ${plan.length} 个文件…`, async () => {
    let ok = 0;
    for (const { file, name } of plan) {
      const buf = new Uint8Array(await file.arrayBuffer());
      await invoke("yfs_write_bytes", { name, data: Array.from(buf) });
      ok += 1;
      status.value = `导入中 ${ok}/${plan.length}…`;
    }
    await refresh();
    const last = files.value.find((f) => f.name === plan[plan.length - 1].name);
    if (last) await selectFile(last);
    status.value = `已导入 ${ok} 个文件`;
  });
}

function openPicker() {
  fileInput.value?.click();
}

function onFilePicked(ev) {
  const input = ev.target;
  if (input.files?.length) importBrowserFiles(input.files);
  input.value = "";
}

function onDragEnter(ev) {
  ev.preventDefault();
  dragging.value = true;
}

function onDragOver(ev) {
  ev.preventDefault();
  dragging.value = true;
}

function onDragLeave(ev) {
  const related = ev.relatedTarget;
  if (!(related instanceof Node) || !ev.currentTarget.contains(related)) {
    dragging.value = false;
  }
}

function onDrop(ev) {
  ev.preventDefault();
  dragging.value = false;
  if (ev.dataTransfer?.files?.length) {
    importBrowserFiles(ev.dataTransfer.files);
  }
}

function enterFolder(name) {
  prefix.value = `${prefix.value}${name}/`;
}

function goCrumb(path) {
  prefix.value = path;
}

function onEditorStatus(msg) {
  status.value = msg;
}

function onEditorError(msg) {
  error.value = msg;
}

async function onImageSaved() {
  await refresh();
  dirty.value = true;
}

onMounted(() => {
  openStore();

  window.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "s") {
      e.preventDefault();
      saveCurrent();
    }
  });
});
</script>

<template>
  <div
    class="yfs"
    :class="{ dragging }"
    @dragenter="onDragEnter"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <header class="top">
      <div class="brand">
        <span class="mark">noteme</span>
        <span class="path" :title="storePath">{{ storePath || "未打开" }}</span>
      </div>
      <div class="actions">
        <button type="button" :disabled="busy" @click="openNewDialog">新建</button>
        <button type="button" :disabled="busy" @click="refresh">刷新</button>
        <button type="button" :disabled="busy" @click="openPicker">导入</button>
        <button type="button" :disabled="busy" @click="compactStore">整理</button>
      </div>
    </header>

    <p v-if="error" class="banner err">{{ error }}</p>
    <p v-else-if="status" class="banner ok">{{ status }}</p>

    <input
      ref="fileInput"
      type="file"
      class="hidden-input"
      multiple
      @change="onFilePicked"
    />

    <div v-if="dialog" class="modal-backdrop" @click.self="closeDialog">
      <div class="modal" role="dialog" aria-modal="true">
        <template v-if="dialog.kind === 'new'">
          <p class="modal-title">新建笔记</p>
          <input
            v-model="newName"
            class="modal-input"
            spellcheck="false"
            @keydown.enter="confirmNew"
          />
          <div class="modal-actions">
            <button type="button" @click="closeDialog">取消</button>
            <button type="button" class="primary" :disabled="busy" @click="confirmNew">
              创建
            </button>
          </div>
        </template>
        <template v-else-if="dialog.kind === 'delete'">
          <p>确定删除「{{ dialog.name }}」？此操作不可撤销。</p>
          <div class="modal-actions">
            <button type="button" :disabled="busy" @click="closeDialog">取消</button>
            <button type="button" class="danger" :disabled="busy" @click="confirmDelete">
              删除
            </button>
          </div>
        </template>
        <template v-else>
          <p>「{{ dialog.name }}」已存在，是否覆盖？</p>
          <div class="modal-actions">
            <button type="button" :disabled="busy" @click="closeDialog">取消</button>
            <button
              type="button"
              class="primary"
              :disabled="busy"
              @click="dialog.resolve(true)"
            >
              覆盖
            </button>
          </div>
        </template>
      </div>
    </div>

    <div class="body">
      <aside class="side">
        <nav class="crumbs">
          <button
            v-for="(c, i) in breadcrumbs"
            :key="c.path + i"
            type="button"
            class="crumb"
            @click="goCrumb(c.path)"
          >
            {{ c.label }}
            <span v-if="i < breadcrumbs.length - 1" class="sep">/</span>
          </button>
        </nav>

        <ul class="list">
          <li v-for="folder in folders" :key="'d-' + folder">
            <button type="button" class="row folder" @click="enterFolder(folder)">
              <span class="icon">▸</span>
              <span class="name">{{ folder }}</span>
            </button>
          </li>
          <li v-for="f in visibleFiles" :key="f.name">
            <button
              type="button"
              class="row file"
              :class="{ active: f.name === selected?.name }"
              @click="selectFile(f)"
            >
              <span class="icon">{{ isMarkdownFile(f.name) ? "#" : "·" }}</span>
              <span class="name">{{ shortName(f.name) }}</span>
              <span class="meta">{{ formatSize(f.size) }}</span>
            </button>
          </li>
          <li v-if="!folders.length && !visibleFiles.length" class="empty">此目录为空</li>
        </ul>
      </aside>

      <section class="main">
        <template v-if="selected && canEdit">
          <div class="editor-bar">
            <div class="file-title">
              <span>{{ shortName(selected.name) }}</span>
              <span v-if="dirty" class="dirty">未保存</span>
            </div>
            <div class="editor-actions">
              <div v-if="isMd" class="mode-toggle" role="group" aria-label="编辑模式">
                <button
                  type="button"
                  :class="{ active: mode === 'edit' }"
                  :disabled="busy"
                  @click="switchMode('edit')"
                >
                  编辑
                </button>
                <button
                  type="button"
                  :class="{ active: mode === 'preview' }"
                  :disabled="busy"
                  @click="switchMode('preview')"
                >
                  预览
                </button>
              </div>
              <button
                type="button"
                class="primary"
                :disabled="busy || !dirty"
                @click="saveCurrent"
              >
                保存
              </button>
              <button type="button" :disabled="busy" @click="deleteFile">删除</button>
            </div>
          </div>

          <div class="editor-pane">
            <MarkdownEditor
              v-if="mode === 'edit'"
              :model-value="content"
              :note-path="selected.name"
              @update:model-value="onContentUpdate"
              @image-saved="onImageSaved"
              @status="onEditorStatus"
              @error="onEditorError"
            />
            <div
              v-else
              class="preview"
              v-html="previewHtml"
            />
          </div>
        </template>

        <template v-else-if="selected">
          <div class="editor-bar">
            <div class="file-title">{{ shortName(selected.name) }}</div>
            <div class="editor-actions">
              <button type="button" :disabled="busy" @click="deleteFile">删除</button>
            </div>
          </div>
          <div class="binary-hint">
            <p>二进制附件，无法在编辑器中打开。</p>
            <p class="muted">{{ formatSize(selected.size) }} · {{ selected.name }}</p>
          </div>
        </template>

        <div v-else class="idle">
          <p class="idle-title">选择或新建一篇笔记</p>
          <p class="muted">左侧文件树浏览 yfs；右侧用 CodeMirror 编辑 Markdown，可切换预览。</p>
          <button type="button" class="primary" :disabled="busy" @click="openNewDialog">
            新建 Markdown
          </button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.yfs {
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

  height: 100vh;
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
  min-width: 0;
}

.mark {
  font-family: var(--serif);
  font-weight: 700;
  letter-spacing: -0.02em;
  font-size: 1.25rem;
}

.path {
  color: var(--muted);
  font-size: 0.78rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--mono);
}

.actions {
  display: flex;
  gap: 0.4rem;
  flex-shrink: 0;
}

button {
  border: 1px solid var(--line);
  background: var(--panel);
  color: var(--ink);
  border-radius: 6px;
  padding: 0.35rem 0.7rem;
  font: inherit;
  font-size: 0.88rem;
  cursor: pointer;
}

button:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
}

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-ink);
}

button.danger {
  background: var(--danger);
  border-color: var(--danger);
  color: #fff;
}

.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  place-items: center;
  background: rgb(28 25 20 / 35%);
  padding: 1rem;
}

.modal {
  width: min(420px, 100%);
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 1.1rem 1.2rem;
  box-shadow: 0 12px 40px rgb(28 25 20 / 18%);
}

.modal p {
  margin: 0 0 1rem;
  line-height: 1.45;
  word-break: break-all;
}

.modal-title {
  font-weight: 600;
  margin-bottom: 0.65rem !important;
}

.modal-input {
  width: 100%;
  box-sizing: border-box;
  margin-bottom: 1rem;
  padding: 0.5rem 0.65rem;
  border: 1px solid var(--line);
  border-radius: 6px;
  font: inherit;
  font-family: var(--mono);
  font-size: 0.9rem;
  background: #fff;
}

.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.45rem;
}

.banner {
  margin: 0;
  padding: 0.4rem 1.1rem;
  font-size: 0.85rem;
}

.banner.err {
  background: var(--danger-bg);
  color: var(--danger);
}

.banner.ok {
  background: var(--ok-bg);
  color: var(--ok);
}

.hidden-input {
  display: none;
}

.body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(200px, 26%) 1fr;
}

.side {
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: color-mix(in srgb, var(--panel) 70%, transparent);
}

.crumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 0.15rem;
  padding: 0.55rem 0.7rem;
  border-bottom: 1px solid var(--line);
}

.crumb {
  border: none;
  background: transparent;
  padding: 0.15rem 0.2rem;
  color: var(--muted);
  font-family: var(--mono);
  font-size: 0.78rem;
}

.crumb:hover {
  color: var(--accent);
}

.sep {
  margin-left: 0.15rem;
  opacity: 0.5;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0.3rem;
  overflow: auto;
  flex: 1;
}

.row {
  width: 100%;
  display: grid;
  grid-template-columns: 1.1rem 1fr auto;
  align-items: center;
  gap: 0.35rem;
  text-align: left;
  border: 1px solid transparent;
  background: transparent;
  border-radius: 6px;
  padding: 0.4rem 0.45rem;
}

.row:hover {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}

.row.active {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  border-color: color-mix(in srgb, var(--accent) 30%, transparent);
}

.row .name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--mono);
  font-size: 0.84rem;
}

.row .meta {
  color: var(--muted);
  font-size: 0.72rem;
}

.row.folder .name {
  font-weight: 600;
}

.icon {
  color: var(--muted);
  font-family: var(--mono);
}

.empty {
  padding: 1rem 0.75rem;
  color: var(--muted);
  font-size: 0.85rem;
}

.main {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--panel);
}

.editor-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.55rem 0.85rem;
  border-bottom: 1px solid var(--line);
  flex-shrink: 0;
}

.file-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
  font-family: var(--mono);
  font-size: 0.9rem;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dirty {
  font-weight: 500;
  font-size: 0.75rem;
  color: var(--accent);
}

.editor-actions {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  flex-shrink: 0;
}

.mode-toggle {
  display: inline-flex;
  border: 1px solid var(--line);
  border-radius: 6px;
  overflow: hidden;
}

.mode-toggle button {
  border: none;
  border-radius: 0;
  background: transparent;
  padding: 0.3rem 0.65rem;
}

.mode-toggle button.active {
  background: var(--accent);
  color: var(--accent-ink);
}

.editor-pane {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.preview {
  height: 100%;
  overflow: auto;
  padding: 1.25rem 1.5rem 2.5rem;
  font-family: var(--serif);
  font-size: 1.05rem;
  line-height: 1.65;
  max-width: 48rem;
}

.preview :deep(h1),
.preview :deep(h2),
.preview :deep(h3) {
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1.25;
  margin: 1.4em 0 0.5em;
}

.preview :deep(h1) {
  font-size: 1.85rem;
  margin-top: 0;
}

.preview :deep(h2) {
  font-size: 1.4rem;
}

.preview :deep(p),
.preview :deep(ul),
.preview :deep(ol) {
  margin: 0.75em 0;
}

.preview :deep(code) {
  font-family: var(--mono);
  font-size: 0.88em;
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  padding: 0.1em 0.35em;
  border-radius: 4px;
}

.preview :deep(pre) {
  font-family: var(--mono);
  font-size: 0.86rem;
  background: #1c1914;
  color: #f3efe6;
  padding: 0.9rem 1rem;
  border-radius: 8px;
  overflow: auto;
}

.preview :deep(pre code) {
  background: none;
  padding: 0;
  color: inherit;
}

.preview :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 4px;
}

.preview :deep(blockquote) {
  margin: 0.85em 0;
  padding-left: 1rem;
  border-left: 3px solid var(--line);
  color: var(--muted);
}

.preview :deep(a) {
  color: var(--accent);
}

.binary-hint,
.idle {
  flex: 1;
  display: grid;
  place-content: center;
  gap: 0.5rem;
  text-align: center;
  padding: 2rem;
}

.idle-title {
  margin: 0;
  font-family: var(--serif);
  font-size: 1.35rem;
  font-weight: 700;
}

.muted {
  margin: 0;
  color: var(--muted);
  font-size: 0.9rem;
  max-width: 28rem;
}

.idle .primary {
  justify-self: center;
  margin-top: 0.5rem;
}

@media (max-width: 720px) {
  .body {
    grid-template-columns: 1fr;
    grid-template-rows: 34% 1fr;
  }

  .side {
    border-right: none;
    border-bottom: 1px solid var(--line);
  }

  .editor-bar {
    flex-wrap: wrap;
  }
}
</style>
