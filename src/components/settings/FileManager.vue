<script setup>
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { isMarkdownFile } from "../../lib/markdown.js";
import { useDocs } from "../../composables/useDocs.js";

const {
  documentRoot,
  files,
  listTruncated,
  busy,
  status,
  error,
  run,
  openRoot,
  refresh,
  pathExists,
  formatSize,
} = useDocs();

const prefix = ref("");
const selected = ref(null);
const dragging = ref(false);
const fileInput = ref(null);

/** @type {import('vue').Ref<null | { kind: 'delete', name: string } | { kind: 'overwrite', name: string, resolve: (ok: boolean) => void } | { kind: 'cleanup', orphans: Array<{ name: string, size: number }>, assetCount: number, docCount: number, truncated: boolean }>} */
const dialog = ref(null);

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
  const p = prefix.value;
  return files.value
    .filter((f) => {
      if (!f.isDir) return false;
      if (!f.name.startsWith(p)) return false;
      const rest = f.name.slice(p.length).replace(/\/$/, "");
      return rest.length > 0 && !rest.includes("/");
    })
    .map((f) => f.name.slice(p.length).replace(/\/$/, ""))
    .sort((a, b) => a.localeCompare(b));
});

const visibleFiles = computed(() => {
  const p = prefix.value;
  return files.value
    .filter((f) => {
      if (f.isDir) return false;
      if (!f.name.startsWith(p)) return false;
      const rest = f.name.slice(p.length);
      return rest.length > 0 && !rest.includes("/");
    })
    .sort((a, b) => a.name.localeCompare(b.name));
});

const totalSize = computed(() =>
  visibleFiles.value.reduce((sum, f) => sum + (f.size || 0), 0),
);

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

async function enterFolder(name) {
  prefix.value = `${prefix.value}${name}/`;
  selected.value = null;
  await run("打开目录…", async () => {
    await refresh(prefix.value);
  });
}

async function goCrumb(path) {
  prefix.value = path;
  selected.value = null;
  await run("打开目录…", async () => {
    await refresh(prefix.value);
  });
}

function selectFile(f) {
  if (f.isDir) return;
  selected.value = f;
}

async function refreshList() {
  await run("刷新…", async () => {
    await refresh(prefix.value);
  });
}

async function importBrowserFiles(fileList) {
  const list = [...fileList].filter((f) => f && f.size >= 0);
  if (!list.length) return;

  const plan = [];
  for (const file of list) {
    const name = targetNameForFile(file);
    if (plan.some((p) => p.name === name)) continue;
    if (await pathExists(name)) {
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
      await invoke("fs_write_bytes", { name, data: Array.from(buf) });
      ok += 1;
      status.value = `导入中 ${ok}/${plan.length}…`;
    }
    await refresh(prefix.value);
    const last = files.value.find((f) => f.name === plan[plan.length - 1].name);
    if (last) selected.value = last;
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

function deleteSelected() {
  if (!selected.value) return;
  askDelete(selected.value.name);
}

async function confirmDelete() {
  const name = dialog.value?.kind === "delete" ? dialog.value.name : null;
  dialog.value = null;
  if (!name) return;
  await run("删除…", async () => {
    await invoke("fs_delete", { name });
    if (selected.value?.name === name) selected.value = null;
    await refresh(prefix.value);
    status.value = "已删除";
  });
}

async function openCleanup() {
  await run("扫描未引用附件…", async () => {
    const result = await invoke("fs_find_orphan_assets");
    const orphans = result?.orphans ?? [];
    if (!orphans.length) {
      status.value = result?.truncated
        ? `扫描未完成（目录过大），未发现可清理项（共 ${result.assetCount ?? 0} 个 .assets 文件）`
        : `没有未引用的 .assets 文件（共扫描 ${result?.assetCount ?? 0} 个附件、${result?.docCount ?? 0} 篇文档）`;
      return;
    }
    dialog.value = {
      kind: "cleanup",
      orphans,
      assetCount: result?.assetCount ?? orphans.length,
      docCount: result?.docCount ?? 0,
      truncated: Boolean(result?.truncated),
    };
    status.value = "";
  });
}

async function confirmCleanup() {
  const orphans =
    dialog.value?.kind === "cleanup" ? dialog.value.orphans : null;
  dialog.value = null;
  if (!orphans?.length) return;
  await run(`清理 ${orphans.length} 个未引用附件…`, async () => {
    let ok = 0;
    for (const f of orphans) {
      await invoke("fs_delete", { name: f.name });
      ok += 1;
      status.value = `清理中 ${ok}/${orphans.length}…`;
    }
    if (selected.value && orphans.some((f) => f.name === selected.value.name)) {
      selected.value = null;
    }
    await refresh(prefix.value);
    status.value = `已清理 ${ok} 个未引用的 .assets 文件`;
  });
}

const cleanupTotalSize = computed(() => {
  if (dialog.value?.kind !== "cleanup") return 0;
  return dialog.value.orphans.reduce((sum, f) => sum + (f.size || 0), 0);
});

onMounted(() => {
  openRoot(prefix.value);
});
</script>

<template>
  <div
    class="file-manager"
    :class="{ dragging }"
    @dragenter="onDragEnter"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <header class="panel-head">
      <div>
        <h2 class="title">文件管理</h2>
        <p class="path" :title="documentRoot">{{ documentRoot || "未配置文档根目录" }}</p>
      </div>
      <div class="actions">
        <button type="button" :disabled="busy" @click="refreshList">刷新</button>
        <button
          type="button"
          :disabled="busy || !documentRoot"
          title="删除 .assets 中未被文档引用的文件"
          @click="openCleanup"
        >
          清理
        </button>
        <button type="button" :disabled="busy || !documentRoot" @click="openPicker">导入</button>
      </div>
    </header>

    <p v-if="error" class="banner err">{{ error }}</p>
    <p v-else-if="status" class="banner ok">{{ status }}</p>
    <p v-else-if="listTruncated" class="banner err">当前目录条目过多，仅显示前 5000 项</p>

    <p class="stats">
      当前目录 {{ visibleFiles.length }} 个文件 · {{ formatSize(totalSize) }}
      <span v-if="dragging" class="drop-hint">松开以导入到当前目录</span>
    </p>

    <input
      ref="fileInput"
      type="file"
      class="hidden-input"
      multiple
      @change="onFilePicked"
    />

    <div v-if="dialog" class="modal-backdrop" @click.self="closeDialog">
      <div
        class="modal"
        :class="{ wide: dialog.kind === 'cleanup' }"
        role="dialog"
        aria-modal="true"
      >
        <template v-if="dialog.kind === 'delete'">
          <p>确定删除「{{ dialog.name }}」？此操作不可撤销。</p>
          <div class="modal-actions">
            <button type="button" :disabled="busy" @click="closeDialog">取消</button>
            <button type="button" class="danger" :disabled="busy" @click="confirmDelete">
              删除
            </button>
          </div>
        </template>
        <template v-else-if="dialog.kind === 'cleanup'">
          <p class="modal-title">清理未引用附件</p>
          <p class="muted cleanup-summary">
            在 {{ dialog.assetCount }} 个 .assets 文件中，发现
            {{ dialog.orphans.length }} 个未被文档引用（约
            {{ formatSize(cleanupTotalSize) }}）。
            <span v-if="dialog.truncated">扫描因目录过大而不完整。</span>
          </p>
          <ul class="orphan-list">
            <li v-for="f in dialog.orphans" :key="f.name">
              <span class="orphan-name">{{ f.name }}</span>
              <span class="orphan-size">{{ formatSize(f.size) }}</span>
            </li>
          </ul>
          <div class="modal-actions">
            <button type="button" :disabled="busy" @click="closeDialog">取消</button>
            <button type="button" class="danger" :disabled="busy" @click="confirmCleanup">
              删除以上文件
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

      <section class="detail">
        <template v-if="selected">
          <h3 class="detail-title">{{ selected.name }}</h3>
          <dl class="meta-grid">
            <dt>大小</dt>
            <dd>{{ formatSize(selected.size) }}</dd>
            <dt>类型</dt>
            <dd>{{ isMarkdownFile(selected.name) ? "Markdown" : "文件" }}</dd>
          </dl>
          <div class="detail-actions">
            <button type="button" class="danger" :disabled="busy" @click="deleteSelected">
              删除
            </button>
          </div>
        </template>
        <div v-else class="idle">
          <p class="idle-title">管理文档文件</p>
          <p class="muted">
            在此导入文件，或浏览删除文档根目录中的文件。日常写笔记请返回主界面。
          </p>
          <button type="button" class="primary" :disabled="busy" @click="openPicker">
            导入文件
          </button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.file-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
  overflow: hidden;
}

.file-manager.dragging {
  outline: 2px dashed var(--accent);
  outline-offset: -6px;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  padding: 1rem 1.1rem 0.75rem;
  border-bottom: 1px solid var(--line);
}

.title {
  margin: 0;
  font-family: var(--serif);
  font-size: 1.2rem;
  font-weight: 700;
}

.path {
  margin: 0.35rem 0 0;
  color: var(--muted);
  font-size: 0.78rem;
  font-family: var(--mono);
  word-break: break-all;
}

.actions {
  display: flex;
  gap: 0.4rem;
  flex-shrink: 0;
}

.stats {
  margin: 0;
  padding: 0.45rem 1.1rem;
  font-size: 0.82rem;
  color: var(--muted);
  border-bottom: 1px solid var(--line);
  display: flex;
  gap: 0.75rem;
  align-items: center;
}

.drop-hint {
  color: var(--accent);
  font-weight: 600;
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

.modal.wide {
  width: min(36rem, 100%);
  max-height: min(80vh, 36rem);
  display: flex;
  flex-direction: column;
}

.modal p {
  margin: 0 0 1rem;
  line-height: 1.45;
  word-break: break-all;
}

.modal-title {
  margin: 0 0 0.4rem;
  font-weight: 650;
}

.cleanup-summary {
  margin: 0 0 0.65rem !important;
  font-size: 0.88rem;
  color: var(--muted);
}

.orphan-list {
  list-style: none;
  margin: 0 0 0.85rem;
  padding: 0.35rem;
  overflow: auto;
  flex: 1;
  min-height: 0;
  max-height: 16rem;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: color-mix(in srgb, var(--bg) 65%, var(--panel));
}

.orphan-list li {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.35rem 0.45rem;
  font-size: 0.82rem;
  border-radius: 5px;
}

.orphan-list li:nth-child(even) {
  background: color-mix(in srgb, var(--line) 35%, transparent);
}

.orphan-name {
  font-family: var(--mono);
  word-break: break-all;
  color: var(--ink);
}

.orphan-size {
  flex-shrink: 0;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}

.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.45rem;
}

.body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(200px, 34%) 1fr;
}

.side {
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: color-mix(in srgb, var(--bg) 55%, transparent);
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

.detail {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 1.25rem 1.35rem;
  overflow: auto;
}

.detail-title {
  margin: 0 0 1rem;
  font-family: var(--mono);
  font-size: 0.95rem;
  word-break: break-all;
}

.meta-grid {
  display: grid;
  grid-template-columns: 4rem 1fr;
  gap: 0.45rem 0.75rem;
  margin: 0 0 1.25rem;
  font-size: 0.9rem;
}

.meta-grid dt {
  margin: 0;
  color: var(--muted);
}

.meta-grid dd {
  margin: 0;
  font-family: var(--mono);
}

.detail-actions {
  display: flex;
  gap: 0.4rem;
}

.idle {
  flex: 1;
  display: grid;
  place-content: center;
  gap: 0.5rem;
  text-align: center;
  padding: 1rem;
}

.idle-title {
  margin: 0;
  font-family: var(--serif);
  font-size: 1.2rem;
  font-weight: 700;
}

.muted {
  margin: 0;
  color: var(--muted);
  font-size: 0.9rem;
  max-width: 26rem;
}

.idle .primary {
  justify-self: center;
  margin-top: 0.5rem;
}

@media (max-width: 720px) {
  .body {
    grid-template-columns: 1fr;
    grid-template-rows: 42% 1fr;
  }

  .side {
    border-right: none;
    border-bottom: 1px solid var(--line);
  }

  .panel-head {
    flex-wrap: wrap;
  }
}
</style>
