<script setup>
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useDocs } from "../../composables/useDocs.js";

const { documentRoot, busy, error, run, loadSettings } = useDocs();

const remoteUrl = ref("");
const httpsToken = ref("");
const commitMessage = ref("");
const gitStatus = ref(null);
const banner = ref({ kind: "", text: "" });

function setBanner(kind, text) {
  banner.value = { kind, text };
}

async function reload() {
  const s = await loadSettings();
  remoteUrl.value = s?.git?.remoteUrl || "";
  httpsToken.value = s?.git?.httpsToken || "";
  try {
    gitStatus.value = await invoke("git_status");
  } catch {
    gitStatus.value = null;
  }
}

async function saveConfig() {
  await run("保存 Git 配置…", async () => {
    const current = await invoke("settings_get");
    const next = {
      ...current,
      git: {
        remoteUrl: remoteUrl.value.trim(),
        httpsToken: httpsToken.value,
      },
    };
    await invoke("settings_set", { settings: next });
    if (documentRoot.value && remoteUrl.value.trim()) {
      try {
        await invoke("git_set_remote", { remoteUrl: remoteUrl.value.trim() });
      } catch {
        // Repo may not exist yet; settings still saved.
      }
    }
    await reload();
    setBanner("ok", "已保存");
  });
}

async function initRepo() {
  await run("初始化仓库…", async () => {
    await saveConfigQuiet();
    gitStatus.value = await invoke("git_init");
    setBanner("ok", "已初始化 git 仓库");
  });
}

async function saveConfigQuiet() {
  const current = await invoke("settings_get");
  await invoke("settings_set", {
    settings: {
      ...current,
      git: {
        remoteUrl: remoteUrl.value.trim(),
        httpsToken: httpsToken.value,
      },
    },
  });
}

async function refreshStatus() {
  await run("读取状态…", async () => {
    gitStatus.value = await invoke("git_status");
    setBanner("ok", gitStatus.value?.message || "已刷新");
  });
}

async function doCommit() {
  await run("提交…", async () => {
    await saveConfigQuiet();
    const msg = await invoke("git_commit", {
      message: commitMessage.value.trim() || null,
    });
    commitMessage.value = "";
    gitStatus.value = await invoke("git_status");
    setBanner("ok", msg);
  });
}

async function doPull() {
  await run("拉取…", async () => {
    await saveConfigQuiet();
    const msg = await invoke("git_pull");
    gitStatus.value = await invoke("git_status");
    setBanner("ok", msg);
  });
}

async function doPush() {
  await run("推送…", async () => {
    await saveConfigQuiet();
    const msg = await invoke("git_push");
    gitStatus.value = await invoke("git_status");
    setBanner("ok", msg);
  });
}

onMounted(() => {
  reload().catch((e) => {
    setBanner("err", typeof e === "string" ? e : e?.message || String(e));
  });
});
</script>

<template>
  <div class="git-sync">
    <header class="panel-head">
      <div>
        <h2 class="title">Git 同步</h2>
        <p class="hint">
          文档根目录即本地仓库。使用 HTTPS Personal Access Token 手动提交、拉取、推送。
        </p>
      </div>
    </header>

    <p v-if="error" class="banner err">{{ error }}</p>
    <p v-else-if="banner.text" class="banner" :class="banner.kind">{{ banner.text }}</p>

    <p v-if="!documentRoot" class="banner err">请先在「文档目录」中选择根目录。</p>

    <section class="card">
      <label class="field">
        <span>Remote URL</span>
        <input
          v-model="remoteUrl"
          type="url"
          spellcheck="false"
          placeholder="https://github.com/user/notes.git"
        />
      </label>
      <label class="field">
        <span>HTTPS Token</span>
        <input
          v-model="httpsToken"
          type="password"
          spellcheck="false"
          autocomplete="off"
          placeholder="ghp_…"
        />
      </label>
      <div class="row-actions">
        <button type="button" :disabled="busy" @click="saveConfig">保存配置</button>
        <button type="button" :disabled="busy || !documentRoot" @click="initRepo">
          初始化仓库
        </button>
      </div>
    </section>

    <section class="card">
      <div class="status-grid">
        <div>
          <div class="label">状态</div>
          <p>{{ gitStatus?.message || "—" }}</p>
        </div>
        <div>
          <div class="label">分支</div>
          <p>{{ gitStatus?.branch || "—" }}</p>
        </div>
        <div>
          <div class="label">远程</div>
          <p class="mono">{{ gitStatus?.remoteUrl || "—" }}</p>
        </div>
        <div>
          <div class="label">ahead / behind</div>
          <p>
            {{ gitStatus?.ahead ?? "—" }} / {{ gitStatus?.behind ?? "—" }}
          </p>
        </div>
      </div>
      <label class="field">
        <span>提交说明（可选）</span>
        <input
          v-model="commitMessage"
          type="text"
          spellcheck="true"
          placeholder="留空则使用默认时间戳说明"
        />
      </label>
      <div class="row-actions">
        <button type="button" :disabled="busy || !documentRoot" @click="refreshStatus">
          状态
        </button>
        <button type="button" :disabled="busy || !documentRoot" @click="doCommit">
          提交
        </button>
        <button type="button" :disabled="busy || !documentRoot" @click="doPull">
          拉取
        </button>
        <button type="button" :disabled="busy || !documentRoot" @click="doPush">
          推送
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.git-sync {
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
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0.9rem 1rem;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: color-mix(in srgb, var(--panel) 80%, transparent);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  font-size: 0.85rem;
}

.field input {
  padding: 0.5rem 0.65rem;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--bg);
  color: var(--ink);
  font-size: 0.92rem;
}

.row-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}

.status-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.75rem 1rem;
}

.status-grid p {
  margin: 0.2rem 0 0;
  font-size: 0.92rem;
  word-break: break-all;
}

.label {
  font-size: 0.78rem;
  color: color-mix(in srgb, var(--ink) 55%, transparent);
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.85rem;
}

@media (max-width: 560px) {
  .status-grid {
    grid-template-columns: 1fr;
  }
}
</style>
