<script setup>
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useDocs } from "../../composables/useDocs.js";

const { busy, error, run, loadSettings } = useDocs();

/** @type {import('vue').Ref<{ name: string, passphrase: string }[]>} */
const labels = ref([]);
const draftName = ref("");
const draftPass = ref("");
const editIndex = ref(-1);
const banner = ref("");

async function reload() {
  const s = await loadSettings();
  labels.value = Array.isArray(s?.cryptoLabels)
    ? s.cryptoLabels.map((l) => ({
        name: l?.name || "",
        passphrase: l?.passphrase || "",
      }))
    : [];
}

async function persist(next) {
  const current = await invoke("settings_get");
  await invoke("settings_set", {
    settings: {
      ...current,
      cryptoLabels: next,
    },
  });
  labels.value = next;
}

function startEdit(i) {
  editIndex.value = i;
  draftName.value = labels.value[i]?.name || "";
  draftPass.value = labels.value[i]?.passphrase || "";
}

function cancelEdit() {
  editIndex.value = -1;
  draftName.value = "";
  draftPass.value = "";
}

async function saveDraft() {
  const name = draftName.value.trim();
  const passphrase = draftPass.value;
  if (!name) {
    banner.value = "标签名不能为空";
    return;
  }
  if (!/^[\w.-]{1,64}$/.test(name)) {
    banner.value = "标签名仅限字母、数字、_ . -（最多 64 字符）";
    return;
  }
  if (!passphrase) {
    banner.value = "口令不能为空";
    return;
  }

  await run("保存标签…", async () => {
    const next = labels.value.map((l) => ({ ...l }));
    const dup = next.findIndex(
      (l, i) => l.name === name && i !== editIndex.value,
    );
    if (dup >= 0) throw new Error(`标签「${name}」已存在`);

    if (editIndex.value >= 0) {
      next[editIndex.value] = { name, passphrase };
    } else {
      next.push({ name, passphrase });
    }
    next.sort((a, b) => a.name.localeCompare(b.name));
    await persist(next);
    cancelEdit();
    banner.value = "已保存";
  });
}

async function removeAt(i) {
  const name = labels.value[i]?.name;
  if (!name) return;
  await run("删除标签…", async () => {
    const next = labels.value.filter((_, idx) => idx !== i);
    await persist(next);
    if (editIndex.value === i) cancelEdit();
    else if (editIndex.value > i) editIndex.value -= 1;
    banner.value = `已删除「${name}」`;
  });
}

onMounted(() => {
  reload();
});
</script>

<template>
  <div class="crypto-labels">
    <header class="panel-head">
      <div>
        <h2 class="title">密码标签</h2>
        <p class="desc">
          为本机保存命名口令。Markdown 中使用
          <code>[passwd:标签:密文]</code>
          时，预览解锁可自动填入对应口令。口令仅存于本地 settings.json。
        </p>
      </div>
    </header>

    <p v-if="error" class="banner err">{{ error }}</p>
    <p v-else-if="banner" class="banner ok">{{ banner }}</p>

    <section class="form">
      <h3 class="sub">{{ editIndex >= 0 ? "编辑标签" : "新增标签" }}</h3>
      <label class="field">
        <span>标签名</span>
        <input
          v-model="draftName"
          type="text"
          spellcheck="false"
          placeholder="例如 work"
          :disabled="busy"
        />
      </label>
      <label class="field">
        <span>口令</span>
        <input
          v-model="draftPass"
          type="password"
          autocomplete="off"
          spellcheck="false"
          :disabled="busy"
          @keydown.enter="saveDraft"
        />
      </label>
      <div class="form-actions">
        <button
          v-if="editIndex >= 0"
          type="button"
          :disabled="busy"
          @click="cancelEdit"
        >
          取消编辑
        </button>
        <button
          type="button"
          class="primary"
          :disabled="busy"
          @click="saveDraft"
        >
          保存
        </button>
      </div>
    </section>

    <section class="list-wrap">
      <h3 class="sub">已保存（{{ labels.length }}）</h3>
      <ul v-if="labels.length" class="list">
        <li v-for="(l, i) in labels" :key="l.name + i" class="row">
          <div class="meta">
            <span class="name">{{ l.name }}</span>
            <span class="pass">••••••••</span>
          </div>
          <div class="row-actions">
            <button type="button" :disabled="busy" @click="startEdit(i)">
              编辑
            </button>
            <button
              type="button"
              class="danger"
              :disabled="busy"
              @click="removeAt(i)"
            >
              删除
            </button>
          </div>
        </li>
      </ul>
      <p v-else class="empty">尚未添加标签。可在加密选区时填写标签名，或在此预先保存。</p>
    </section>
  </div>
</template>

<style scoped>
.crypto-labels {
  max-width: 40rem;
}

.panel-head {
  margin-bottom: 0.85rem;
}

.title {
  margin: 0;
  font-family: var(--serif);
  font-size: 1.2rem;
  font-weight: 700;
}

.desc {
  margin: 0.4rem 0 0;
  color: var(--muted);
  font-size: 0.88rem;
  line-height: 1.45;
}

.desc code {
  font-family: var(--mono);
  font-size: 0.82rem;
  padding: 0.05rem 0.3rem;
  border-radius: 4px;
  background: color-mix(in srgb, var(--line) 45%, transparent);
}

.banner {
  margin: 0 0 0.75rem;
  padding: 0.4rem 0.65rem;
  border-radius: 6px;
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

.form,
.list-wrap {
  margin-bottom: 1.25rem;
  padding: 0.85rem 0.95rem;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
}

.sub {
  margin: 0 0 0.65rem;
  font-size: 0.95rem;
  font-weight: 650;
}

.field {
  display: grid;
  gap: 0.25rem;
  margin-bottom: 0.55rem;
  font-size: 0.85rem;
  color: var(--muted);
}

.field input {
  width: 100%;
  box-sizing: border-box;
  padding: 0.4rem 0.55rem;
  border: 1px solid var(--line);
  border-radius: 6px;
  font: inherit;
  font-size: 0.9rem;
  background: var(--panel);
  color: var(--ink);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.4rem;
  margin-top: 0.35rem;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  padding: 0.45rem 0.2rem;
  border-top: 1px solid var(--line);
}

.row:first-child {
  border-top: none;
}

.meta {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
}

.name {
  font-family: var(--mono);
  font-size: 0.9rem;
  font-weight: 600;
}

.pass {
  font-size: 0.78rem;
  color: var(--muted);
  letter-spacing: 0.08em;
}

.row-actions {
  display: flex;
  gap: 0.35rem;
  flex-shrink: 0;
}

.empty {
  margin: 0;
  color: var(--muted);
  font-size: 0.88rem;
}
</style>
