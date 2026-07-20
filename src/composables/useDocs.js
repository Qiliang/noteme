import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

const documentRoot = ref("");
const files = ref([]);
const listTruncated = ref(false);
const busy = ref(false);
const status = ref("");
const error = ref("");
let openPromise = null;

/**
 * Shared document-root filesystem state for Workspace / File Manager.
 * Listing is shallow (one directory at a time) via `refresh(prefix)`.
 */
export function useDocs() {
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

  /**
   * @param {string} [dirPrefix] directory relative to root, e.g. `""` or `"notes/"`
   */
  async function refresh(dirPrefix = "") {
    const prefix = (dirPrefix || "").replace(/^\/+/, "");
    const result = await invoke("fs_list", { prefix });
    files.value = result?.entries ?? result ?? [];
    listTruncated.value = Boolean(result?.truncated);
  }

  async function loadSettings() {
    const s = await invoke("settings_get");
    documentRoot.value = s?.documentRoot || "";
    return s;
  }

  async function openRoot(dirPrefix = "") {
    if (documentRoot.value) {
      await refresh(dirPrefix);
      return documentRoot.value;
    }
    if (!openPromise) {
      openPromise = run("打开文档目录…", async () => {
        const s = await loadSettings();
        if (!s?.documentRoot) {
          throw new Error("尚未配置文档根目录，请先在设置中选择");
        }
        documentRoot.value = s.documentRoot;
        await refresh(dirPrefix);
      }).finally(() => {
        openPromise = null;
      });
    }
    await openPromise;
    return documentRoot.value;
  }

  async function pathExists(name) {
    try {
      await invoke("fs_stat", { name });
      return true;
    } catch {
      return false;
    }
  }

  function formatSize(n) {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }

  return {
    documentRoot,
    files,
    listTruncated,
    busy,
    status,
    error,
    run,
    loadSettings,
    openRoot,
    refresh,
    pathExists,
    formatSize,
  };
}
