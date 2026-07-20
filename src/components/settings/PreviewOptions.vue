<script setup>
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  DEFAULT_PREVIEW_FEATURES,
  clampRenderScalePercent,
  usePreviewFeatures,
} from "../../composables/usePreviewFeatures.js";

const { features, resetPreviewFeatures } = usePreviewFeatures();

const DOCS = "https://satteri.bruits.org/docs/features/";

function openDocs(ev) {
  ev.preventDefault();
  openUrl(DOCS);
}

const toggles = [
  {
    key: "gfm",
    label: "GFM",
    desc: "表格、脚注、删除线、任务列表、自动链接",
  },
  {
    key: "frontmatter",
    label: "Frontmatter",
    desc: "文档开头的 YAML / TOML 元数据块",
  },
  {
    key: "math",
    label: "Math",
    desc: "行内 $...$ 与块级 $$...$$ 数学公式",
  },
  {
    key: "mermaid",
    label: "Mermaid",
    desc: "本地渲染 ```mermaid 图表；闭合围栏可写 ```{width=200px} 指定样式",
  },
  {
    key: "excalidraw",
    label: "Excalidraw",
    desc: "渲染 ![…](*.excalidraw)，支持 {width=300px}；⌘/Ctrl + 点击浮动编辑",
  },
  {
    key: "headingAttributes",
    label: "Heading attributes",
    desc: "标题花括号属性，如 ## Title {#id .class}",
  },
  {
    key: "directive",
    label: "Directives",
    desc: ":::name / ::name / :name 指令（默认不渲染，需插件）",
  },
  {
    key: "superscript",
    label: "Superscript",
    desc: "^text^ 上标",
  },
  {
    key: "subscript",
    label: "Subscript",
    desc: "~text~ 下标（会关闭 GFM 单波浪删除线）",
  },
  {
    key: "wikilinks",
    label: "Wikilinks",
    desc: "[[Target]] 与 [[Target|Label]]",
  },
  {
    key: "definitionList",
    label: "Definition lists",
    desc: "术语 + 冒号定义列表（Pandoc 风格）",
  },
  {
    key: "smartPunctuation",
    label: "Smart punctuation",
    desc: "弯引号、破折号、省略号",
  },
];

function isDefault() {
  return Object.keys(DEFAULT_PREVIEW_FEATURES).every(
    (k) => features[k] === DEFAULT_PREVIEW_FEATURES[k],
  );
}

function clampAutoInterval() {
  let n = Number(features.autoRenderIntervalSec);
  if (!Number.isFinite(n) || n < 1) n = 1;
  if (n > 300) n = 300;
  features.autoRenderIntervalSec = Math.round(n);
}

function clampScale() {
  features.renderScalePercent = clampRenderScalePercent(
    features.renderScalePercent,
  );
}
</script>

<template>
  <div class="preview-options">
    <header class="panel-head">
      <div>
        <h2 class="title">预览选项</h2>
        <p class="lead">
          调整预览行为与 Sätteri
          <code>features</code>。详见
          <a :href="DOCS" @click="openDocs">官方文档</a>。
        </p>
      </div>
      <button type="button" :disabled="isDefault()" @click="resetPreviewFeatures">
        恢复默认
      </button>
    </header>

    <div class="auto-render">
      <label class="row interval-row">
        <span class="text">
          <span class="label">自动渲染周期</span>
          <span class="desc">
            预览打开时每隔若干秒刷新；内容未变化则跳过。默认 3 秒。
          </span>
        </span>
        <span class="interval-field">
          <input
            v-model.number="features.autoRenderIntervalSec"
            type="number"
            min="1"
            max="300"
            step="1"
            @change="clampAutoInterval"
            @blur="clampAutoInterval"
          />
          <span class="unit">秒</span>
        </span>
      </label>
      <label class="row interval-row">
        <span class="text">
          <span class="label">渲染缩放比例</span>
          <span class="desc">预览内容缩放，范围 10%～200%。默认 100%。</span>
        </span>
        <span class="interval-field">
          <input
            v-model.number="features.renderScalePercent"
            type="number"
            min="10"
            max="200"
            step="5"
            @change="clampScale"
            @blur="clampScale"
          />
          <span class="unit">%</span>
        </span>
      </label>
    </div>

    <ul class="feature-list">
      <li v-for="item in toggles" :key="item.key" class="feature">
        <label class="row">
          <input v-model="features[item.key]" type="checkbox" />
          <span class="text">
            <span class="label">{{ item.label }}</span>
            <span class="desc">{{ item.desc }}</span>
          </span>
        </label>

        <div v-if="item.key === 'math' && features.math" class="sub">
          <label class="row nested">
            <input v-model="features.mathSingleDollar" type="checkbox" />
            <span class="text">
              <span class="label">Single-dollar math</span>
              <span class="desc">
                启用 $...$ 行内公式；关闭后仅保留 $$...$$，避免金额被误解析
              </span>
            </span>
          </label>
        </div>

        <div
          v-if="item.key === 'smartPunctuation' && features.smartPunctuation"
          class="sub"
        >
          <label class="row nested">
            <input v-model="features.smartQuotes" type="checkbox" />
            <span class="text">
              <span class="label">Quotes</span>
              <span class="desc">弯引号</span>
            </span>
          </label>
          <label class="row nested">
            <input v-model="features.smartDashes" type="checkbox" />
            <span class="text">
              <span class="label">Dashes</span>
              <span class="desc">-- / --- 破折号</span>
            </span>
          </label>
          <label class="row nested">
            <input v-model="features.smartEllipses" type="checkbox" />
            <span class="text">
              <span class="label">Ellipses</span>
              <span class="desc">... 省略号</span>
            </span>
          </label>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.preview-options {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
  overflow: hidden;
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  padding: 1rem 1.1rem 0.85rem;
  border-bottom: 1px solid var(--line);
}

.title {
  margin: 0;
  font-family: var(--serif);
  font-size: 1.2rem;
  font-weight: 700;
}

.lead {
  margin: 0.4rem 0 0;
  color: var(--muted);
  font-size: 0.88rem;
  line-height: 1.45;
  max-width: 36rem;
}

.lead code {
  font-family: var(--mono);
  font-size: 0.84em;
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  padding: 0.05em 0.3em;
  border-radius: 3px;
}

.lead a {
  color: var(--accent);
}

.auto-render {
  border-bottom: 1px solid var(--line);
}

.interval-row {
  align-items: center;
  cursor: default;
}

.interval-row:hover {
  background: transparent;
}

.interval-field {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  flex-shrink: 0;
  margin-left: auto;
}

.interval-field input {
  width: 4.5rem;
  margin: 0;
  padding: 0.3rem 0.45rem;
  border: 1px solid var(--line);
  border-radius: 6px;
  font: inherit;
  font-family: var(--mono);
  font-size: 0.88rem;
  background: #fff;
  accent-color: var(--accent);
}

.unit {
  color: var(--muted);
  font-size: 0.82rem;
}

.feature-list {
  list-style: none;
  margin: 0;
  padding: 0.4rem 0;
  overflow: auto;
  flex: 1;
}

.feature {
  border-bottom: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
}

.feature:last-child {
  border-bottom: none;
}

.row {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.75rem 1.1rem;
  cursor: pointer;
}

.row:hover {
  background: color-mix(in srgb, var(--accent) 5%, transparent);
}

.row.nested {
  padding: 0.45rem 1.1rem 0.45rem 2.6rem;
}

.row input[type="checkbox"] {
  margin-top: 0.2rem;
  accent-color: var(--accent);
  width: 1rem;
  height: 1rem;
  flex-shrink: 0;
}

.text {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  min-width: 0;
}

.label {
  font-weight: 600;
  font-size: 0.92rem;
}

.desc {
  color: var(--muted);
  font-size: 0.82rem;
  line-height: 1.4;
}

.sub {
  padding-bottom: 0.35rem;
}
</style>
