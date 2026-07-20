import { reactive, watch } from "vue";

const STORAGE_KEY = "noteme.previewFeatures";

/** App defaults (Sätteri defaults + math on for notes). */
export const DEFAULT_PREVIEW_FEATURES = {
  gfm: true,
  frontmatter: true,
  math: true,
  mathSingleDollar: true,
  headingAttributes: false,
  directive: false,
  superscript: false,
  subscript: false,
  wikilinks: false,
  definitionList: false,
  smartPunctuation: false,
  smartQuotes: true,
  smartDashes: true,
  smartEllipses: true,
  /** App-level: not passed to Sätteri. */
  mermaid: true,
  /** App-level: render ![…](*.excalidraw) (and legacy fences) via local SVG. */
  excalidraw: true,
  /** Seconds between auto preview refreshes (app-level). */
  autoRenderIntervalSec: 3,
  /** Preview zoom percent (10–200). */
  renderScalePercent: 100,
};

/** Clamp preview zoom to 10–200. */
export function clampRenderScalePercent(n) {
  let v = Number(n);
  if (!Number.isFinite(v)) v = 100;
  return Math.min(200, Math.max(10, Math.round(v)));
}

const state = reactive({ ...DEFAULT_PREVIEW_FEATURES });
let loaded = false;

function load() {
  if (loaded) return;
  loaded = true;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return;
    for (const key of Object.keys(DEFAULT_PREVIEW_FEATURES)) {
      const def = DEFAULT_PREVIEW_FEATURES[key];
      const val = parsed[key];
      if (typeof def === "boolean" && typeof val === "boolean") {
        state[key] = val;
      } else if (typeof def === "number" && typeof val === "number" && Number.isFinite(val)) {
        state[key] =
          key === "renderScalePercent" ? clampRenderScalePercent(val) : val;
      }
    }
  } catch {
    /* ignore corrupt storage */
  }
}

function persist() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...state }));
}

load();
watch(state, persist, { deep: true });

/**
 * Shape expected by Sätteri `markdownToHtml({ features })`.
 * @see https://satteri.bruits.org/docs/features/
 */
export function toSatteriFeatures(features = state) {
  return {
    gfm: features.gfm,
    frontmatter: features.frontmatter,
    math: features.math
      ? { singleDollarTextMath: features.mathSingleDollar }
      : false,
    headingAttributes: features.headingAttributes,
    directive: features.directive,
    superscript: features.superscript,
    subscript: features.subscript,
    wikilinks: features.wikilinks,
    definitionList: features.definitionList,
    smartPunctuation: features.smartPunctuation
      ? {
          quotes: features.smartQuotes,
          dashes: features.smartDashes,
          ellipses: features.smartEllipses,
        }
      : false,
  };
}

export function resetPreviewFeatures() {
  Object.assign(state, DEFAULT_PREVIEW_FEATURES);
}

export function usePreviewFeatures() {
  load();
  return {
    features: state,
    resetPreviewFeatures,
    clampRenderScalePercent,
    toSatteriFeatures: () => toSatteriFeatures(state),
  };
}
