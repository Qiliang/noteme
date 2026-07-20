<script setup>
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import MarkdownEditor from "./MarkdownEditor.vue";
import ExcalidrawEditor from "./ExcalidrawEditor.vue";
import {
  isExcalidrawFile,
  isMarkdownFile,
  isTextEditable,
  renderMarkdown,
  setHtmlPreservingImages,
} from "../lib/markdown.js";
import { createExcalidrawAsset, readExcalidrawDoc } from "../lib/excalidraw.js";
import {
  formatPasswdMarker,
  lockWidgetHtml,
  PASSWD_REVEAL_SEC,
  plainWidgetHtml,
} from "../lib/passwd.js";
import { useDocs } from "../composables/useDocs.js";
import {
  clampRenderScalePercent,
  usePreviewFeatures,
} from "../composables/usePreviewFeatures.js";
import iconDiagram from "../assets/icons/diagram.svg?raw";
import iconTable from "../assets/icons/table.svg?raw";
import iconEditor from "../assets/icons/editor.svg?raw";
import iconEditorOff from "../assets/icons/editor-off.svg?raw";
import iconPreview from "../assets/icons/preview.svg?raw";
import iconPreviewOff from "../assets/icons/preview-off.svg?raw";
import iconSave from "../assets/icons/save.svg?raw";
import iconLock from "../assets/icons/lock.svg?raw";

const emit = defineEmits(["open-settings"]);

const {
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
const { features } = usePreviewFeatures();

const LAYOUT_KEY = "noteme.layout";

function loadLayout() {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return { sideOpen: true, previewOpen: true, editorOpen: true };
    const parsed = JSON.parse(raw);
    return {
      sideOpen: parsed?.sideOpen !== false,
      previewOpen: parsed?.previewOpen !== false,
      editorOpen: parsed?.editorOpen !== false,
    };
  } catch {
    return { sideOpen: true, previewOpen: true, editorOpen: true };
  }
}

const layoutInit = loadLayout();
const sideOpen = ref(layoutInit.sideOpen);
const previewOpen = ref(layoutInit.previewOpen);
const editorOpen = ref(layoutInit.editorOpen);

watch([sideOpen, previewOpen, editorOpen], () => {
  localStorage.setItem(
    LAYOUT_KEY,
    JSON.stringify({
      sideOpen: sideOpen.value,
      previewOpen: previewOpen.value,
      editorOpen: editorOpen.value,
    }),
  );
});

const prefix = ref("");
const selected = ref(null);
const content = ref("");
const previewHtml = ref("");
/** @type {import('vue').Ref<HTMLElement | null>} */
const previewEl = ref(null);
/** Loading indicator in the preview pane (UI only). */
const previewLoading = ref(false);
/** True while a render job is in flight (may be silent). */
const previewRendering = ref(false);
/** Source string last successfully rendered into previewHtml. */
const lastRenderedSource = ref(null);
const dirty = ref(false);
const loadToken = ref(0);
const renderToken = ref(0);
const newName = ref("untitled.md");
const renameName = ref("");
const tableCols = ref(3);
const tableRows = ref(3);

/** Skip / defer auto-render while the user is actively typing. */
const TYPING_IDLE_MS = 550;
/** Only show the progress overlay if render takes longer than this. */
const SHOW_LOADING_AFTER_MS = 200;

let autoRenderTimer = null;
let idlePreviewTimer = null;
let loadingDelayTimer = null;
let lastInputAt = 0;
let pendingSilentRender = false;
let pendingForceRender = false;

function yieldToMain() {
  if (typeof scheduler !== "undefined" && typeof scheduler.yield === "function") {
    return scheduler.yield();
  }
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** Let the loading UI paint before heavy markdown/mermaid work. */
function paintFrame() {
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  });
}

function clearLoadingDelay() {
  if (loadingDelayTimer != null) {
    clearTimeout(loadingDelayTimer);
    loadingDelayTimer = null;
  }
}

function clearIdlePreviewTimer() {
  if (idlePreviewTimer != null) {
    clearTimeout(idlePreviewTimer);
    idlePreviewTimer = null;
  }
}

/** Run a silent preview refresh shortly after typing stops. */
function schedulePreviewWhenIdle() {
  clearIdlePreviewTimer();
  idlePreviewTimer = setTimeout(() => {
    idlePreviewTimer = null;
    void refreshPreview({ silent: true });
  }, TYPING_IDLE_MS);
}

/** @type {import('vue').Ref<null | { kind: 'delete', name: string } | { kind: 'overwrite', name: string, resolve: (ok: boolean) => void } | { kind: 'new' } | { kind: 'rename', from: string } | { kind: 'table' } | { kind: 'encrypt', plain: string }>} */
const dialog = ref(null);

const encryptPassphrase = ref("");
const encryptLabel = ref("");
/** @type {import('vue').Ref<{ name: string, passphrase: string }[]>} */
const cryptoLabels = ref([]);

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

function isHiddenName(name) {
  return name.startsWith(".");
}

const folders = computed(() => {
  const p = prefix.value;
  return files.value
    .filter((f) => {
      if (!f.isDir) return false;
      if (!f.name.startsWith(p)) return false;
      const rest = f.name.slice(p.length).replace(/\/$/, "");
      if (!rest.length || rest.includes("/")) return false;
      return !isHiddenName(rest);
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
      if (!rest.length || rest.includes("/")) return false;
      return !isHiddenName(rest);
    })
    .sort((a, b) => a.name.localeCompare(b.name));
});

const canEdit = computed(() => {
  if (!selected.value) return false;
  return isTextEditable(selected.value.name, selected.value.fileType);
});

const isMd = computed(() => selected.value && isMarkdownFile(selected.value.name));
const isExcalidraw = computed(
  () => selected.value && isExcalidrawFile(selected.value.name),
);

const previewStale = computed(
  () => Boolean(isMd.value) && content.value !== lastRenderedSource.value,
);

const previewScaleStyle = computed(() => {
  const scale = clampRenderScalePercent(features.renderScalePercent) / 100;
  return {
    zoom: String(scale),
  };
});

function clampPreviewScale() {
  features.renderScalePercent = clampRenderScalePercent(
    features.renderScalePercent,
  );
}

/** @type {import('vue').Ref<InstanceType<typeof MarkdownEditor> | null>} */
const mdEditor = ref(null);
/** True after the selected .excalidraw file body has been read into `content`. */
const excalidrawReady = ref(false);

function shortName(full) {
  if (!prefix.value) return full;
  return full.startsWith(prefix.value) ? full.slice(prefix.value.length) : full;
}

async function selectFile(f) {
  if (selected.value?.name === f.name) return;
  if (dirty.value && selected.value) {
    await saveCurrent();
  }
  selected.value = f;
  clearLoadingDelay();
  clearIdlePreviewTimer();
  previewLoading.value = false;
  previewRendering.value = false;
  lastRenderedSource.value = null;
  await loadSelected();
}

async function loadSelected() {
  const f = selected.value;
  const token = ++loadToken.value;
  content.value = "";
  previewHtml.value = "";
  excalidrawReady.value = false;
  clearLoadingDelay();
  clearIdlePreviewTimer();
  previewLoading.value = false;
  previewRendering.value = false;
  lastRenderedSource.value = null;
  dirty.value = false;
  if (!f || !isTextEditable(f.name, f.fileType)) return;

  await run("读取…", async () => {
    const text = await invoke("fs_read", { name: f.name });
    if (token !== loadToken.value) return;
    content.value = text;
    dirty.value = false;
    if (isExcalidrawFile(f.name)) excalidrawReady.value = true;
  });
  if (token === loadToken.value) await refreshPreview({ force: true });
}

function onContentUpdate(v) {
  content.value = v;
  dirty.value = true;
  lastInputAt = Date.now();
}

async function saveCurrent() {
  if (!selected.value || !canEdit.value) return;
  await run("保存…", async () => {
    await invoke("fs_write", {
      name: selected.value.name,
      content: content.value,
    });
    dirty.value = false;
    await refresh(prefix.value);
    syncSelected();
  });
}

function syncSelected() {
  if (!selected.value) return;
  const next = files.value.find((f) => f.name === selected.value.name);
  if (next) selected.value = next;
}

/**
 * Render preview if open. Skips when source is unchanged.
 * @param {{ force?: boolean, silent?: boolean }} [opts]
 * - force: manual / initial load — ignore typing-idle deferral
 * - silent: no loading overlay (auto refresh); default true when !force
 */
async function refreshPreview(opts = {}) {
  const force = opts.force === true;
  const silent = opts.silent ?? !force;

  if (!isMd.value || !previewOpen.value) return;
  if (!force && content.value === lastRenderedSource.value) return;

  if (previewRendering.value) {
    pendingSilentRender = true;
    if (force) pendingForceRender = true;
    return;
  }

  if (!force && Date.now() - lastInputAt < TYPING_IDLE_MS) {
    schedulePreviewWhenIdle();
    return;
  }

  const source = content.value;
  const notePath = selected.value?.name;
  const token = ++renderToken.value;
  previewRendering.value = true;
  pendingSilentRender = false;
  pendingForceRender = false;

  clearLoadingDelay();
  const showLoadingSoon = !silent;
  if (showLoadingSoon) {
    if (!previewHtml.value) {
      previewLoading.value = true;
      await nextTick();
      await paintFrame();
    } else {
      loadingDelayTimer = setTimeout(() => {
        loadingDelayTimer = null;
        if (token === renderToken.value) previewLoading.value = true;
      }, SHOW_LOADING_AFTER_MS);
    }
  }

  try {
    await yieldToMain();
    if (token !== renderToken.value) return;
    if (!force && Date.now() - lastInputAt < TYPING_IDLE_MS) {
      schedulePreviewWhenIdle();
      return;
    }

    const html = await renderMarkdown(source, { notePath });
    if (token !== renderToken.value) return;
    if (html === previewHtml.value) {
      lastRenderedSource.value = source;
      return;
    }
    previewHtml.value = html;
    lastRenderedSource.value = source;
  } catch (e) {
    if (token !== renderToken.value) return;
    error.value = typeof e === "string" ? e : e?.message || String(e);
  } finally {
    if (token === renderToken.value) {
      clearLoadingDelay();
      previewLoading.value = false;
      previewRendering.value = false;
      if (pendingForceRender) {
        pendingForceRender = false;
        pendingSilentRender = false;
        void refreshPreview({ force: true });
      } else if (pendingSilentRender) {
        pendingSilentRender = false;
        void refreshPreview({ silent: true });
      }
    }
  }
}

function toggleSide() {
  sideOpen.value = !sideOpen.value;
}

function togglePreview() {
  if (previewOpen.value) {
    previewOpen.value = false;
    if (!editorOpen.value) editorOpen.value = true;
  } else {
    previewOpen.value = true;
  }
}

function toggleEditor() {
  if (editorOpen.value) {
    editorOpen.value = false;
    if (!previewOpen.value) previewOpen.value = true;
  } else {
    editorOpen.value = true;
  }
}

function stopAutoRender() {
  if (autoRenderTimer != null) {
    clearInterval(autoRenderTimer);
    autoRenderTimer = null;
  }
}

function restartAutoRender() {
  stopAutoRender();
  let sec = Number(features.autoRenderIntervalSec);
  if (!Number.isFinite(sec) || sec < 1) return;
  sec = Math.min(300, Math.round(sec));
  autoRenderTimer = setInterval(() => {
    void refreshPreview({ silent: true });
  }, sec * 1000);
}

watch(previewOpen, (open) => {
  if (open) void refreshPreview({ force: true });
});

// Keep the preview DOM in sync; reuse <img> nodes with the same src to avoid flash.
watch([previewHtml, previewEl], async ([html, el]) => {
  clearPasswdRevealTimers();
  if (!el) return;
  setHtmlPreservingImages(el, html ?? "");
  await nextTick();
  void prefillLabeledPasswdInputs(el);
});

watch(
  () => features.autoRenderIntervalSec,
  () => restartAutoRender(),
);

async function confirmDelete() {
  const name = dialog.value?.kind === "delete" ? dialog.value.name : null;
  dialog.value = null;
  if (!name) return;
  await run("删除…", async () => {
    await invoke("fs_delete", { name });
    if (selected.value?.name === name) {
      selected.value = null;
      content.value = "";
      previewHtml.value = "";
      clearLoadingDelay();
      clearIdlePreviewTimer();
      previewLoading.value = false;
      previewRendering.value = false;
      lastRenderedSource.value = null;
      dirty.value = false;
    }
    await refresh(prefix.value);
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

  if (await pathExists(name)) {
    const ok = await askOverwrite(name);
    if (!ok) return;
  }

  await run("创建…", async () => {
    const initial = `# ${name.split("/").pop().replace(/\.md$/i, "")}\n\n`;
    await invoke("fs_write", { name, content: initial });
    await refresh(prefix.value);
    let f = files.value.find((x) => x.name === name);
    if (!f) {
      try {
        f = await invoke("fs_stat", { name });
      } catch {
        f = null;
      }
    }
    if (f) await selectFile(f);
    status.value = "已创建";
  });
}

function openRenameDialog(file) {
  renameName.value = shortName(file.name);
  dialog.value = { kind: "rename", from: file.name };
}

async function confirmRename() {
  const from = dialog.value?.kind === "rename" ? dialog.value.from : null;
  let name = renameName.value.trim().replace(/^\/+/, "");
  if (!from || !name) return;
  if (name.includes("/") || name.includes("\\")) {
    error.value = "名称不能包含路径分隔符";
    return;
  }
  const to = `${prefix.value}${name}`;
  dialog.value = null;
  if (to === from) return;

  await run("重命名…", async () => {
    await invoke("fs_rename", { old: from, new: to });
    if (selected.value?.name === from) {
      selected.value = { ...selected.value, name: to };
    }
    await refresh(prefix.value);
    syncSelected();
  });
}

async function enterFolder(name) {
  prefix.value = `${prefix.value}${name}/`;
  await run("打开目录…", async () => {
    await refresh(prefix.value);
  });
}

async function goCrumb(path) {
  prefix.value = path;
  await run("打开目录…", async () => {
    await refresh(prefix.value);
  });
}

function onEditorStatus(msg) {
  status.value = msg;
}

function onEditorError(msg) {
  error.value = msg;
}

/** @type {import('vue').Ref<null | { path: string, content: string }>} */
const excalOverlay = ref(null);
/** @type {import('vue').Ref<InstanceType<typeof ExcalidrawEditor> | null>} */
const overlayEditor = ref(null);

/**
 * Open Excalidraw in a full-window floating editor (does not leave the note).
 * @param {string} relOrAbs
 */
async function openExcalOverlay(relOrAbs) {
  if (!selected.value) {
    error.value = `Excalidraw：无效路径 ${relOrAbs ?? ""}`;
    return;
  }
  await run("打开图…", async () => {
    const { path: docPath, text } = await readExcalidrawDoc(
      selected.value.name,
      relOrAbs,
    );
    excalOverlay.value = { path: docPath, content: text };
    status.value = "⌘/Ctrl + 点击可再次打开";
  });
}

/** Save scene from the live editor API, then close the overlay. */
async function finishExcalOverlay() {
  const current = excalOverlay.value;
  if (!current) return;

  const json =
    overlayEditor.value?.serialize?.() ?? current.content ?? "";

  excalOverlay.value = null;
  await run("保存图…", async () => {
    await invoke("fs_write", { name: current.path, content: json });
    if (isMd.value && previewOpen.value) {
      await refreshPreview({ force: true, silent: true });
    }
    status.value = "图已保存";
  });
}

async function insertExcalidraw() {
  if (!selected.value || !isMd.value) return;
  if (!editorOpen.value) editorOpen.value = true;
  await nextTick();
  // Editor view mounts in onMounted; wait a few ticks if we just opened the pane.
  for (let i = 0; i < 10 && !mdEditor.value; i++) await nextTick();

  const noteName = selected.value.name;
  await run("插入 Excalidraw…", async () => {
    const { markdown: md, path } = await createExcalidrawAsset(noteName);
    const inserted = mdEditor.value?.insertAtCursor?.(md);
    if (!inserted) {
      // Fallback when CodeMirror is not ready: append into the bound doc.
      const sep = !content.value || content.value.endsWith("\n") ? "" : "\n";
      content.value = `${content.value}${sep}${md}\n`;
      dirty.value = true;
    }
    await nextTick();
    await invoke("fs_write", {
      name: noteName,
      content: content.value,
    });
    dirty.value = false;
    await refresh(prefix.value);
    syncSelected();
    if (previewOpen.value) {
      await refreshPreview({ force: true, silent: true });
    }
    status.value = `已插入 Excalidraw（${path}）`;
  });
}

function openTableDialog() {
  if (!selected.value || !isMd.value) return;
  tableCols.value = 3;
  tableRows.value = 3;
  dialog.value = { kind: "table" };
}

/**
 * @param {number} cols
 * @param {number} rows total rows including header
 */
function buildMarkdownTable(cols, rows) {
  const c = Math.min(20, Math.max(1, Math.round(Number(cols)) || 3));
  const r = Math.min(50, Math.max(1, Math.round(Number(rows)) || 3));
  const header = Array.from({ length: c }, (_, i) => `列${i + 1}`);
  const sep = Array.from({ length: c }, () => "---");
  const empty = Array.from({ length: c }, () => " ");
  const lines = [
    `| ${header.join(" | ")} |`,
    `| ${sep.join(" | ")} |`,
  ];
  for (let i = 1; i < r; i++) {
    lines.push(`| ${empty.join(" | ")} |`);
  }
  return `${lines.join("\n")}\n`;
}

async function confirmInsertTable() {
  if (dialog.value?.kind !== "table") return;
  const cols = tableCols.value;
  const rows = tableRows.value;
  dialog.value = null;
  if (!selected.value || !isMd.value) return;
  if (!editorOpen.value) editorOpen.value = true;
  await nextTick();
  mdEditor.value?.insertAtCursor(buildMarkdownTable(cols, rows));
  dirty.value = true;
}

async function loadCryptoLabels() {
  try {
    const s = await invoke("settings_get");
    cryptoLabels.value = Array.isArray(s?.cryptoLabels) ? s.cryptoLabels : [];
  } catch {
    cryptoLabels.value = [];
  }
}

async function openEncryptDialog() {
  if (!selected.value || !isMd.value) return;
  if (!editorOpen.value) editorOpen.value = true;
  await nextTick();
  const sel = mdEditor.value?.getSelection?.();
  const plain = (sel?.text ?? "").trim();
  if (!plain) {
    error.value = "请先选中要加密的文本";
    return;
  }
  await loadCryptoLabels();
  encryptPassphrase.value = "";
  encryptLabel.value = "";
  dialog.value = { kind: "encrypt", plain };
}

function onEncryptLabelPick(name) {
  encryptLabel.value = name;
  const hit = cryptoLabels.value.find((l) => l.name === name);
  if (hit?.passphrase) encryptPassphrase.value = hit.passphrase;
}

async function confirmEncrypt() {
  if (dialog.value?.kind !== "encrypt") return;
  const plain = dialog.value.plain;
  const passphrase = encryptPassphrase.value;
  const label = encryptLabel.value.trim();
  if (!passphrase) {
    error.value = "请输入口令";
    return;
  }
  dialog.value = null;
  await run("加密选区…", async () => {
    const cipher = await invoke("crypto_encrypt", { plaintext: plain, passphrase });
    const marker = formatPasswdMarker(cipher, label);
    const ok = mdEditor.value?.replaceSelection?.(marker);
    if (!ok) throw new Error("编辑器未就绪，无法插入密文");
    dirty.value = true;
    status.value = label ? `已加密（标签：${label}）` : "已加密";
    if (previewOpen.value) await refreshPreview({ force: true, silent: true });
  });
}

/** @type {Map<string, ReturnType<typeof setTimeout>>} */
const passwdRevealTimers = new Map();

function clearPasswdRevealTimers() {
  for (const id of passwdRevealTimers.values()) clearTimeout(id);
  passwdRevealTimers.clear();
}

/**
 * Prefill saved label passphrases into lock inputs (does not auto-unlock).
 * @param {HTMLElement | null | undefined} root
 */
async function prefillLabeledPasswdInputs(root) {
  if (!root) return;
  const locks = [...root.querySelectorAll(".passwd-lock[data-passwd-label]")];
  if (!locks.length) return;
  await loadCryptoLabels();
  for (const lock of locks) {
    const label = lock.getAttribute("data-passwd-label") || "";
    const hit = cryptoLabels.value.find((l) => l.name === label);
    if (!hit?.passphrase) continue;
    const input = lock.querySelector(".passwd-lock-input");
    if (input instanceof HTMLInputElement && !input.value) {
      input.value = hit.passphrase;
    }
  }
}

/**
 * @param {HTMLElement} plainEl
 */
function relockPasswdPlain(plainEl) {
  if (!plainEl?.isConnected) return;
  const cipher = plainEl.getAttribute("data-passwd-cipher") || "";
  const label = plainEl.getAttribute("data-passwd-label") || "";
  if (!cipher) return;
  const wrap = document.createElement("span");
  wrap.innerHTML = lockWidgetHtml(label, cipher);
  const lock = wrap.firstElementChild;
  if (lock) plainEl.replaceWith(lock);
  else plainEl.remove();
  passwdRevealTimers.delete(cipher);
}

/**
 * @param {HTMLElement} plainEl
 * @param {string} cipher
 */
function schedulePasswdRelock(plainEl, cipher) {
  const prev = passwdRevealTimers.get(cipher);
  if (prev) clearTimeout(prev);
  const id = setTimeout(() => {
    passwdRevealTimers.delete(cipher);
    relockPasswdPlain(plainEl);
  }, PASSWD_REVEAL_SEC * 1000);
  passwdRevealTimers.set(cipher, id);
}

/**
 * @param {Element | null} lockEl
 */
async function unlockPasswdLock(lockEl) {
  if (!lockEl || !(lockEl instanceof HTMLElement)) return;
  const cipher = lockEl.getAttribute("data-passwd-cipher") || "";
  const label = lockEl.getAttribute("data-passwd-label") || "";
  const input = lockEl.querySelector(".passwd-lock-input");
  const errEl = lockEl.querySelector(".passwd-lock-err");
  let passphrase =
    input instanceof HTMLInputElement ? input.value : "";

  if (!passphrase && label) {
    await loadCryptoLabels();
    const hit = cryptoLabels.value.find((l) => l.name === label);
    if (hit?.passphrase) passphrase = hit.passphrase;
  }
  if (!cipher) return;
  if (!passphrase) {
    if (errEl) {
      errEl.hidden = false;
      errEl.textContent = label ? "请输入口令，或在设置中配置该标签" : "请输入口令";
    }
    return;
  }
  if (errEl) {
    errEl.hidden = true;
    errEl.textContent = "";
  }
  try {
    const plain = await invoke("crypto_decrypt", { ciphertext: cipher, passphrase });
    const wrap = document.createElement("span");
    wrap.innerHTML = plainWidgetHtml(plain, cipher, label, PASSWD_REVEAL_SEC);
    const span = wrap.firstElementChild;
    if (!(span instanceof HTMLElement)) return;
    lockEl.replaceWith(span);
    schedulePasswdRelock(span, cipher);
  } catch (e) {
    const msg = typeof e === "string" ? e : e?.message || String(e);
    if (errEl) {
      errEl.hidden = false;
      errEl.textContent = msg || "解锁失败";
    }
  }
}

function onPreviewClick(e) {
  const t = e.target;
  if (!(t instanceof Element)) return;

  const unlockBtn = t.closest(".passwd-lock-btn");
  if (unlockBtn) {
    e.preventDefault();
    void unlockPasswdLock(unlockBtn.closest(".passwd-lock"));
    return;
  }

  if (!(e.metaKey || e.ctrlKey)) return;
  const diagram = t.closest(".excalidraw-diagram");
  if (!diagram) return;
  const path = diagram.getAttribute("data-excalidraw-path");
  if (!path) return;
  e.preventDefault();
  void openExcalOverlay(path);
}

function onPreviewKeydown(e) {
  if (e.key !== "Enter") return;
  const t = e.target;
  if (!(t instanceof HTMLInputElement) || !t.classList.contains("passwd-lock-input")) {
    return;
  }
  e.preventDefault();
  void unlockPasswdLock(t.closest(".passwd-lock"));
}

function fileIcon(name) {
  if (isMarkdownFile(name)) return "#";
  if (isExcalidrawFile(name)) return "◇";
  return "·";
}

async function refreshList() {
  await run("刷新…", async () => {
    await refresh(prefix.value);
    syncSelected();
  });
}

/** @type {import('vue').Ref<{ x: number, y: number, file: object | null } | null>} */
const treeMenu = ref(null);

function openTreeMenu(e, file = null) {
  e.preventDefault();
  treeMenu.value = { x: e.clientX, y: e.clientY, file };
}

function closeTreeMenu() {
  treeMenu.value = null;
}

function onTreeMenuNew() {
  closeTreeMenu();
  openNewDialog();
}

function onTreeMenuRefresh() {
  closeTreeMenu();
  refreshList();
}

function onTreeMenuRename() {
  const file = treeMenu.value?.file;
  closeTreeMenu();
  if (file) openRenameDialog(file);
}

function onTreeMenuDelete() {
  const file = treeMenu.value?.file;
  closeTreeMenu();
  if (file) askDelete(file.name);
}

function onTreeMenuPointerDown(e) {
  if (!treeMenu.value) return;
  const t = e.target;
  if (t instanceof Element && t.closest(".tree-menu")) return;
  closeTreeMenu();
}

async function onImageSaved() {
  await refresh(prefix.value);
  syncSelected();
  dirty.value = true;
}

function onKeydown(e) {
  if (e.key === "Escape") {
    if (excalOverlay.value) {
      void finishExcalOverlay();
      return;
    }
    if (treeMenu.value) {
      closeTreeMenu();
      return;
    }
  }
  if ((e.metaKey || e.ctrlKey) && e.key === "s") {
    e.preventDefault();
    if (excalOverlay.value) {
      void finishExcalOverlay();
      return;
    }
    saveCurrent();
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b") {
    e.preventDefault();
    toggleSide();
  }
}

onMounted(() => {
  openRoot(prefix.value).then(() => syncSelected());
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("pointerdown", onTreeMenuPointerDown, true);
  restartAutoRender();
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("pointerdown", onTreeMenuPointerDown, true);
  stopAutoRender();
  clearLoadingDelay();
  clearIdlePreviewTimer();
  clearPasswdRevealTimers();
});
</script>

<template>
  <div class="workspace">
    <p v-if="error" class="banner err">{{ error }}</p>
    <p v-else-if="status" class="banner ok">{{ status }}</p>
    <p v-else-if="listTruncated" class="banner err">
      当前目录条目过多，仅显示前 5000 项
    </p>

    <div
      v-if="dialog"
      class="modal-backdrop"
      @click.self="dialog.kind !== 'new' && dialog.kind !== 'rename' && dialog.kind !== 'table' && dialog.kind !== 'encrypt' && closeDialog()"
    >
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
        <template v-else-if="dialog.kind === 'rename'">
          <p class="modal-title">重命名</p>
          <input
            v-model="renameName"
            class="modal-input"
            spellcheck="false"
            @keydown.enter="confirmRename"
          />
          <div class="modal-actions">
            <button type="button" @click="closeDialog">取消</button>
            <button type="button" class="primary" :disabled="busy" @click="confirmRename">
              确定
            </button>
          </div>
        </template>
        <template v-else-if="dialog.kind === 'table'">
          <p class="modal-title">插入表格</p>
          <label class="modal-field">
            <span>列数</span>
            <input
              v-model.number="tableCols"
              class="modal-input"
              type="number"
              min="1"
              max="20"
              @keydown.enter="confirmInsertTable"
            />
          </label>
          <label class="modal-field">
            <span>行数</span>
            <input
              v-model.number="tableRows"
              class="modal-input"
              type="number"
              min="1"
              max="50"
              @keydown.enter="confirmInsertTable"
            />
          </label>
          <div class="modal-actions">
            <button type="button" @click="closeDialog">取消</button>
            <button type="button" class="primary" @click="confirmInsertTable">
              插入
            </button>
          </div>
        </template>
        <template v-else-if="dialog.kind === 'encrypt'">
          <p class="modal-title">加密选区</p>
          <p class="muted modal-hint">明文不会写入文档，仅插入 [passwd:…] 密文标记。</p>
          <label class="modal-field">
            <span>口令</span>
            <input
              v-model="encryptPassphrase"
              class="modal-input"
              type="password"
              autocomplete="off"
              spellcheck="false"
              @keydown.enter="confirmEncrypt"
            />
          </label>
          <label class="modal-field">
            <span>标签（可选）</span>
            <input
              v-model="encryptLabel"
              class="modal-input"
              list="crypto-label-options"
              placeholder="例如 work"
              spellcheck="false"
              @change="onEncryptLabelPick(encryptLabel)"
            />
            <datalist id="crypto-label-options">
              <option v-for="l in cryptoLabels" :key="l.name" :value="l.name" />
            </datalist>
          </label>
          <div class="modal-actions">
            <button type="button" @click="closeDialog">取消</button>
            <button type="button" class="primary" :disabled="busy" @click="confirmEncrypt">
              加密并插入
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

    <div
      v-if="treeMenu"
      class="tree-menu"
      role="menu"
      :style="{ left: treeMenu.x + 'px', top: treeMenu.y + 'px' }"
    >
      <button
        type="button"
        role="menuitem"
        :disabled="busy"
        @click="onTreeMenuNew"
      >
        新建
      </button>
      <button
        v-if="treeMenu.file"
        type="button"
        role="menuitem"
        :disabled="busy"
        @click="onTreeMenuRename"
      >
        重命名
      </button>
      <button
        v-if="treeMenu.file"
        type="button"
        role="menuitem"
        class="danger-item"
        :disabled="busy"
        @click="onTreeMenuDelete"
      >
        删除
      </button>
      <button
        type="button"
        role="menuitem"
        :disabled="busy"
        @click="onTreeMenuRefresh"
      >
        刷新
      </button>
    </div>

    <div class="body" :class="{ 'side-closed': !sideOpen }">
      <aside v-if="sideOpen" class="side">
        <div class="side-head">
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
        </div>

        <ul class="list" @contextmenu="openTreeMenu($event, null)">
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
              @contextmenu.stop="openTreeMenu($event, f)"
            >
              <span class="icon">{{ fileIcon(f.name) }}</span>
              <span class="name">{{ shortName(f.name) }}</span>
              <span class="meta">{{ formatSize(f.size) }}</span>
            </button>
          </li>
          <li v-if="!folders.length && !visibleFiles.length" class="empty">此目录为空</li>
        </ul>

        <div class="side-foot">
          <button
            type="button"
            class="icon-btn settings-btn"
            data-tip="设置"
            aria-label="设置"
            @click="emit('open-settings')"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <circle cx="12" cy="12" r="3" />
              <path
                d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
              />
            </svg>
          </button>
        </div>
      </aside>

      <section class="main">
        <template v-if="selected && canEdit">
          <div class="editor-bar">
            <div class="file-title">
              <span>{{ shortName(selected.name) }}</span>
              <span v-if="dirty" class="dirty">未保存</span>
            </div>
            <div class="editor-actions">
              <button
                v-if="isMd"
                type="button"
                class="icon-btn"
                data-tip="插入图"
                aria-label="插入图"
                :disabled="busy"
                @click="insertExcalidraw"
              >
                <span class="icon-svg" v-html="iconDiagram" />
              </button>
              <button
                v-if="isMd"
                type="button"
                class="icon-btn"
                data-tip="插入表格"
                aria-label="插入表格"
                :disabled="busy"
                @click="openTableDialog"
              >
                <span class="icon-svg" v-html="iconTable" />
              </button>
              <button
                v-if="isMd"
                type="button"
                class="icon-btn"
                data-tip="加密选区"
                aria-label="加密选区"
                :disabled="busy"
                @click="openEncryptDialog"
              >
                <span class="icon-svg" v-html="iconLock" />
              </button>
              <button
                v-if="isMd"
                type="button"
                class="icon-btn"
                :data-tip="editorOpen ? '隐藏编辑' : '显示编辑'"
                :aria-label="editorOpen ? '隐藏编辑' : '显示编辑'"
                :disabled="busy"
                @click="toggleEditor"
              >
                <span
                  class="icon-svg"
                  v-html="editorOpen ? iconEditor : iconEditorOff"
                />
              </button>
              <button
                v-if="isMd"
                type="button"
                class="icon-btn"
                :data-tip="previewOpen ? '隐藏预览' : '显示预览'"
                :aria-label="previewOpen ? '隐藏预览' : '显示预览'"
                :disabled="busy"
                @click="togglePreview"
              >
                <span
                  class="icon-svg"
                  v-html="previewOpen ? iconPreview : iconPreviewOff"
                />
              </button>
              <label
                v-if="isMd && previewOpen"
                class="scale-field"
                title="渲染缩放比例 10%～200%"
              >
                <span class="scale-label">缩放</span>
                <input
                  v-model.number="features.renderScalePercent"
                  type="number"
                  min="10"
                  max="200"
                  step="5"
                  :disabled="busy"
                  @change="clampPreviewScale"
                  @blur="clampPreviewScale"
                />
                <span class="scale-unit">%</span>
              </label>
              <button
                v-if="isMd && previewOpen"
                type="button"
                :disabled="busy || previewRendering || !previewStale"
                @click="refreshPreview({ force: true })"
              >
                渲染
              </button>
              <button
                type="button"
                class="icon-btn primary"
                data-tip="保存"
                aria-label="保存"
                :disabled="busy || !dirty"
                @click="saveCurrent"
              >
                <span class="icon-svg" v-html="iconSave" />
              </button>
            </div>
          </div>

          <div
            class="editor-pane"
            :class="{ 'has-preview': isMd && previewOpen && editorOpen }"
          >
            <div v-if="editorOpen || !isMd" class="pane-edit">
              <ExcalidrawEditor
                v-if="isExcalidraw && excalidrawReady"
                :key="selected.name"
                :model-value="content"
                @update:model-value="onContentUpdate"
              />
              <MarkdownEditor
                v-else
                ref="mdEditor"
                :model-value="content"
                :note-path="selected.name"
                @update:model-value="onContentUpdate"
                @image-saved="onImageSaved"
                @status="onEditorStatus"
                @error="onEditorError"
                @open-excalidraw="openExcalOverlay"
              />
            </div>
            <div v-if="isMd && previewOpen" class="pane-preview">
              <div
                v-if="previewLoading"
                class="preview-loading"
                :class="{ overlay: Boolean(previewHtml) }"
                role="status"
                aria-live="polite"
                aria-busy="true"
              >
                <div class="preview-progress" aria-hidden="true">
                  <div class="preview-progress-bar" />
                </div>
                <p class="preview-loading-text">正在渲染预览…</p>
              </div>
              <div
                ref="previewEl"
                class="preview"
                :style="previewScaleStyle"
                @click="onPreviewClick"
                @keydown="onPreviewKeydown"
              />
            </div>
          </div>
        </template>

        <template v-else-if="selected">
          <div class="editor-bar">
            <div class="file-title">{{ shortName(selected.name) }}</div>
          </div>
          <div class="binary-hint">
            <p>二进制附件，无法在编辑器中打开。</p>
            <p class="muted">{{ formatSize(selected.size) }} · {{ selected.name }}</p>
          </div>
        </template>

        <div v-else class="idle">
          <p class="idle-title">选择或新建一篇笔记</p>
          <p class="muted">左侧浏览文档树，右侧编辑 Markdown；导入与整理请到「设置 → 文件管理」。</p>
          <button type="button" class="primary" :disabled="busy" @click="openNewDialog">
            新建 Markdown
          </button>
        </div>
      </section>
    </div>

    <div
      v-if="excalOverlay"
      class="excal-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="编辑 Excalidraw"
    >
      <div class="excal-overlay-bar">
        <div class="excal-overlay-title">
          <span class="excal-overlay-mark">Excalidraw</span>
          <span class="excal-overlay-path">{{ excalOverlay.path }}</span>
        </div>
        <div class="excal-overlay-actions">
          <button type="button" class="primary" :disabled="busy" @click="finishExcalOverlay">
            完成
          </button>
        </div>
      </div>
      <div class="excal-overlay-body">
        <ExcalidrawEditor
          ref="overlayEditor"
          :key="excalOverlay.path"
          :model-value="excalOverlay.content"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.workspace {
  height: 100%;
  display: flex;
  flex-direction: column;
  background:
    radial-gradient(1200px 500px at 10% -10%, #e7f2ee 0%, transparent 55%),
    linear-gradient(180deg, #f7f2e8 0%, var(--bg) 100%);
  color: var(--ink);
  font-family: var(--sans);
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

.modal-hint {
  margin-top: -0.35rem !important;
  font-size: 0.85rem;
  word-break: normal !important;
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

.modal-field {
  display: grid;
  grid-template-columns: 5.5rem 1fr;
  align-items: center;
  gap: 0.65rem;
  margin-bottom: 0.75rem;
  font-size: 0.9rem;
  color: var(--muted);
}

.modal-field .modal-input {
  margin-bottom: 0;
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
  grid-template-columns: 168px 1fr;
}

.body.side-closed {
  grid-template-columns: 1fr;
}

.side {
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
  background: color-mix(in srgb, var(--panel) 70%, transparent);
}

.side-head {
  border-bottom: 1px solid var(--line);
}

.side-head .crumbs {
  border-bottom: none;
}

.side-foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0.35rem;
  border-top: 1px solid var(--line);
}

.settings-btn {
  width: 1.85rem;
  height: 1.85rem;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
}

.settings-btn:hover {
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}

.icon-btn[data-tip] {
  position: relative;
}

.icon-btn[data-tip]::after {
  content: attr(data-tip);
  position: absolute;
  top: calc(100% + 6px);
  left: 50%;
  z-index: 30;
  padding: 0.22rem 0.45rem;
  border-radius: 5px;
  background: color-mix(in srgb, var(--ink) 92%, #000);
  color: var(--panel);
  font-size: 0.72rem;
  line-height: 1.2;
  white-space: nowrap;
  pointer-events: none;
  opacity: 0;
  transform: translateX(-50%) translateY(-2px);
  transition:
    opacity 60ms ease 80ms,
    transform 60ms ease 80ms;
}

.icon-btn[data-tip]:hover::after,
.icon-btn[data-tip]:focus-visible::after {
  opacity: 1;
  transform: translateX(-50%) translateY(0);
}

.side-foot .icon-btn[data-tip]::after {
  top: auto;
  bottom: calc(100% + 6px);
}

.tree-menu {
  position: fixed;
  z-index: 50;
  min-width: 7.5rem;
  padding: 0.25rem;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--panel);
  box-shadow: 0 8px 24px rgb(28 25 20 / 16%);
  display: flex;
  flex-direction: column;
  gap: 0.1rem;
}

.tree-menu button {
  width: 100%;
  text-align: left;
  border: none;
  border-radius: 5px;
  background: transparent;
  padding: 0.4rem 0.55rem;
  font: inherit;
  font-size: 0.85rem;
  color: var(--ink);
  cursor: pointer;
}

.tree-menu button:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  color: var(--accent);
}

.tree-menu button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.tree-menu button.danger-item {
  color: var(--danger);
}

.tree-menu button.danger-item:hover:not(:disabled) {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}

.crumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 0.15rem;
  padding: 0.55rem 0.45rem 0.55rem 0.55rem;
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
  grid-template-columns: 1rem 1fr auto;
  align-items: center;
  gap: 0.25rem;
  text-align: left;
  border: 1px solid transparent;
  background: transparent;
  border-radius: 6px;
  padding: 0.35rem 0.35rem;
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
  font-size: 0.78rem;
}

.row .meta {
  color: var(--muted);
  font-size: 0.68rem;
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
  gap: 0.35rem;
  flex-shrink: 0;
}

.editor-actions .icon-btn {
  position: relative;
  width: 1.9rem;
  height: 1.9rem;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  color: var(--ink);
}

.editor-actions .icon-btn.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-ink);
}

.editor-actions .icon-btn.primary:hover:not(:disabled) {
  border-color: var(--accent);
  filter: brightness(1.05);
}

.editor-actions .icon-svg {
  display: inline-flex;
  width: 16px;
  height: 16px;
  line-height: 0;
}

.editor-actions .icon-svg :deep(svg) {
  display: block;
  width: 16px;
  height: 16px;
}

.scale-field {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  margin: 0;
  color: var(--muted);
  font-size: 0.82rem;
  white-space: nowrap;
}

.scale-field input {
  width: 3.6rem;
  margin: 0;
  padding: 0.28rem 0.35rem;
  border: 1px solid var(--line);
  border-radius: 6px;
  font: inherit;
  font-family: var(--mono);
  font-size: 0.85rem;
  background: var(--panel);
  color: var(--ink);
}

.scale-field input:disabled {
  opacity: 0.5;
}

.scale-label,
.scale-unit {
  user-select: none;
}

.editor-pane {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
}

.pane-edit {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.editor-pane.has-preview .pane-edit {
  border-right: 1px solid var(--line);
}

.pane-preview {
  position: relative;
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.preview-loading {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.85rem;
  padding: 2rem;
}

.preview-loading.overlay {
  position: absolute;
  inset: 0;
  z-index: 2;
  height: auto;
  background: color-mix(in srgb, var(--panel) 72%, transparent);
  backdrop-filter: blur(2px);
}

.preview-progress {
  width: min(16rem, 70%);
  height: 3px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--line) 85%, transparent);
  overflow: hidden;
}

.preview-progress-bar {
  height: 100%;
  width: 38%;
  border-radius: inherit;
  background: var(--accent);
  animation: preview-progress-slide 1.05s ease-in-out infinite;
}

.preview-loading-text {
  margin: 0;
  color: var(--muted);
  font-family: var(--sans);
  font-size: 0.9rem;
}

@keyframes preview-progress-slide {
  0% {
    transform: translateX(-120%);
  }
  100% {
    transform: translateX(320%);
  }
}

.preview {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 1rem 1.15rem 2rem;
  font-family: var(--serif);
  font-size: 1.02rem;
  line-height: 1.65;
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

.preview :deep(.mermaid-diagram) {
  margin: 1.1em 0;
  text-align: left;
  overflow-x: auto;
  max-width: 100%;
  box-sizing: border-box;
}

.preview :deep(.mermaid-diagram svg) {
  display: block;
  max-width: 100%;
  height: auto;
  margin-left: 0;
  margin-right: auto;
}

.preview :deep(.mermaid-diagram[style*="width"] svg) {
  width: 100%;
}

.preview :deep(.mermaid-error) {
  margin: 1.1em 0;
  padding: 0.85rem 1rem;
  border: 1px solid color-mix(in srgb, var(--line) 80%, transparent);
  border-radius: 8px;
  background: color-mix(in srgb, var(--bg) 70%, transparent);
}

.preview :deep(.mermaid-error-msg) {
  margin: 0 0 0.65em;
  color: var(--muted);
  font-family: var(--sans);
  font-size: 0.88rem;
}

.preview :deep(.mermaid-error-source) {
  margin: 0;
}

.preview :deep(.excalidraw-diagram) {
  margin: 1rem 0;
  overflow-x: auto;
  cursor: pointer;
  max-width: 100%;
}

.preview :deep(.excalidraw-diagram svg) {
  display: block;
  max-width: 100%;
  height: auto;
  margin-left: 0;
  margin-right: auto;
}

.excal-overlay {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  flex-direction: column;
  background: var(--panel);
}

.excal-overlay-bar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.55rem 0.9rem;
  border-bottom: 1px solid var(--line);
  background: color-mix(in srgb, var(--panel) 92%, transparent);
}

.excal-overlay-title {
  display: flex;
  align-items: baseline;
  gap: 0.65rem;
  min-width: 0;
}

.excal-overlay-mark {
  flex-shrink: 0;
  font-weight: 700;
  color: var(--accent);
  font-size: 0.92rem;
}

.excal-overlay-path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--muted);
  font-family: var(--mono);
  font-size: 0.8rem;
}

.excal-overlay-actions {
  display: flex;
  gap: 0.4rem;
  flex-shrink: 0;
}

.excal-overlay-body {
  flex: 1;
  min-height: 0;
}

.preview :deep(.excalidraw-error) {
  margin: 1rem 0;
  padding: 0.75rem 0.9rem;
  border: 1px solid #e2b6b6;
  border-radius: 6px;
  background: #fdf5f5;
}

.preview :deep(.passwd-lock) {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.35rem;
  margin: 0.15rem 0;
  padding: 0.35rem 0.5rem;
  border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--line));
  border-radius: 8px;
  background: color-mix(in srgb, var(--accent) 8%, var(--panel));
  vertical-align: middle;
}

.preview :deep(.passwd-lock-hint) {
  font-size: 0.82rem;
  color: var(--muted);
  margin-right: 0.15rem;
}

.preview :deep(.passwd-lock-input) {
  width: 8.5rem;
  margin: 0;
  padding: 0.25rem 0.4rem;
  border: 1px solid var(--line);
  border-radius: 5px;
  font: inherit;
  font-size: 0.85rem;
  background: var(--panel);
  color: var(--ink);
}

.preview :deep(.passwd-lock-btn) {
  padding: 0.25rem 0.55rem;
  font-size: 0.82rem;
}

.preview :deep(.passwd-lock-err) {
  flex-basis: 100%;
  font-size: 0.78rem;
  color: var(--danger);
}

.preview :deep(.passwd-plain) {
  position: relative;
  display: inline-block;
  max-width: 100%;
  padding: 0.2rem 0.45rem 0.2rem 0.4rem;
  border-radius: 6px;
  background: color-mix(in srgb, var(--accent) 10%, var(--panel));
  border: 1px dashed color-mix(in srgb, var(--accent) 40%, var(--line));
  white-space: pre-wrap;
  word-break: break-word;
}

.preview :deep(.passwd-plain-meta) {
  margin-left: 0.45rem;
  font-size: 0.72rem;
  color: var(--muted);
  user-select: none;
}

.preview :deep(.excalidraw-error-msg) {
  margin: 0;
  color: #a33;
  font-size: 0.9rem;
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

.preview :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 1em 0;
  font-family: var(--sans);
  font-size: 0.95rem;
}

.preview :deep(th),
.preview :deep(td) {
  border: 1px solid var(--line);
  padding: 0.45rem 0.7rem;
  text-align: left;
  vertical-align: top;
}

.preview :deep(th) {
  background: color-mix(in srgb, var(--accent) 8%, var(--panel));
  font-weight: 600;
}

.preview :deep(tr:nth-child(even) td) {
  background: color-mix(in srgb, var(--bg) 55%, transparent);
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
    grid-template-rows: 30% 1fr;
  }

  .body.side-closed {
    grid-template-columns: 1fr;
    grid-template-rows: 1fr;
  }

  .side {
    border-right: none;
    border-bottom: 1px solid var(--line);
  }

  .editor-pane.has-preview {
    flex-direction: column;
  }

  .editor-pane.has-preview .pane-edit {
    border-right: none;
    border-bottom: 1px solid var(--line);
    max-height: 50%;
  }

  .editor-bar {
    flex-wrap: wrap;
  }
}
</style>
