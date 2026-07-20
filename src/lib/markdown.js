import { markdownToHtml } from "satteri";
import { invoke } from "@tauri-apps/api/core";
import {
  usePreviewFeatures,
  toSatteriFeatures,
} from "../composables/usePreviewFeatures.js";
import {
  noteDir,
  resolveDocPath,
} from "./docPath.js";
import { renderMermaidInHtml } from "./mermaid.js";
import { renderExcalidrawInHtml } from "./excalidraw.js";
import { renderPasswdInHtml } from "./passwd.js";

export {
  docPathCandidates,
  normalizeDocHref,
  noteDir,
  resolveDocPath,
} from "./docPath.js";

function yieldToMain() {
  if (typeof scheduler !== "undefined" && typeof scheduler.yield === "function") {
    return scheduler.yield();
  }
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** HTML attributes kept as element attrs; everything else becomes CSS. */
const IMG_HTML_ATTRS = new Set([
  "alt",
  "title",
  "id",
  "class",
  "loading",
  "decoding",
  "crossorigin",
  "referrerpolicy",
]);

/**
 * Parse `width=300px height=200 #id .cls class=foo style="..."` inside `{}`.
 * @param {string} body
 */
function parseImageAttrBlock(body) {
  /** @type {Record<string, string>} */
  const htmlAttrs = {};
  /** @type {string[]} */
  const classes = [];
  /** @type {string[]} */
  const styles = [];

  const tokenRe =
    /(?:#([\w-]+))|(?:\.([\w-]+))|(?:([\w-]+)=(?:"([^"]*)"|'([^']*)'|([^\s"'}]+)))|(?:([\w-]+)(?=\s|$))/g;

  let m;
  while ((m = tokenRe.exec(body)) !== null) {
    if (m[1]) {
      htmlAttrs.id = m[1];
      continue;
    }
    if (m[2]) {
      classes.push(m[2]);
      continue;
    }
    const key = m[3] || m[7];
    if (!key) continue;
    const raw = m[4] ?? m[5] ?? m[6] ?? "";
    const k = key.toLowerCase();

    if (k === "class") {
      classes.push(...raw.split(/\s+/).filter(Boolean));
      continue;
    }
    if (k === "style") {
      if (raw.trim()) styles.push(raw.trim().replace(/;?\s*$/, ""));
      continue;
    }
    if (IMG_HTML_ATTRS.has(k)) {
      htmlAttrs[k] = raw;
      continue;
    }

    // Presentational keys → CSS. Bare numbers for width/height get px.
    let value = raw;
    if (
      (k === "width" || k === "height") &&
      value &&
      /^\d+(\.\d+)?$/.test(value)
    ) {
      value = `${value}px`;
    }
    if (value) styles.push(`${key}: ${value}`);
  }

  return { htmlAttrs, classes, styles };
}

/**
 * @param {string} tag `<img …>` HTML
 * @param {string} name
 */
function getImgAttr(tag, name) {
  const re = new RegExp(`\\b${name}="([^"]*)"`, "i");
  return tag.match(re)?.[1] ?? "";
}

/**
 * Set or replace an attribute on an <img> tag string.
 * @param {string} tag
 * @param {string} name
 * @param {string} value
 */
function setImgAttr(tag, name, value) {
  const re = new RegExp(`\\s${name}="[^"]*"`, "i");
  if (re.test(tag)) return tag.replace(re, ` ${name}="${value}"`);
  return tag.replace(/\/?>$/, ` ${name}="${value}"$&`);
}

/**
 * Apply trailing `{width=300px …}` attribute blocks onto preceding <img> tags.
 * Supports: `![alt](url) {width=300px height=200}` and no-space `{…}`.
 * @param {string} html
 */
export function applyImageAttrs(html) {
  if (!html || !html.includes("{")) return html;

  return html.replace(
    /(<img\b[^>]*>)(\s*)\{([^{}]*)\}/gi,
    (full, imgTag, _ws, body) => {
      const { htmlAttrs, classes, styles } = parseImageAttrBlock(body);
      let tag = imgTag;

      for (const [k, v] of Object.entries(htmlAttrs)) {
        if (k === "class") {
          classes.push(...String(v).split(/\s+/).filter(Boolean));
        } else {
          tag = setImgAttr(tag, k, v);
        }
      }

      if (classes.length) {
        const prev = getImgAttr(tag, "class").split(/\s+/).filter(Boolean);
        tag = setImgAttr(
          tag,
          "class",
          [...new Set([...prev, ...classes])].join(" "),
        );
      }

      if (styles.length) {
        const prev = getImgAttr(tag, "style").trim().replace(/;?\s*$/, "");
        const merged = [prev, ...styles].filter(Boolean).join("; ");
        tag = setImgAttr(tag, "style", `${merged};`);
      }

      return tag;
    },
  );
}

/**
 * CommonMark only closes a fence with a bare ``` line. Rewrite
 * ```{width=200px} into a real closer plus a following `{…}` paragraph
 * so styles can be applied to Mermaid (and similar) blocks.
 * @param {string} source
 */
export function normalizeFenceClosingAttrs(source) {
  if (!source || !source.includes("{")) return source ?? "";
  return source.replace(
    /^([ \t]{0,3})(`{3,}|~{3,})[ \t]*\{([^{}\n]*)\}[ \t]*$/gm,
    (_, indent, fence, body) =>
      `${indent}${fence}\n\n${indent}{${String(body).trim()}}`,
  );
}

/**
 * Apply a following `{width=200px …}` paragraph onto block elements
 * (e.g. `.mermaid-diagram` after fence attrs were normalized).
 * @param {string} html
 * @param {string} selector
 */
export function applyTrailingBlockAttrs(html, selector) {
  if (!html || !html.includes("{") || !selector) return html;

  const doc = new DOMParser().parseFromString(
    `<div id="nm-root">${html}</div>`,
    "text/html",
  );
  const root = doc.getElementById("nm-root");
  if (!root) return html;

  let changed = false;
  for (const el of [...root.querySelectorAll(selector)]) {
    const next = el.nextElementSibling;
    if (!next || next.tagName !== "P") continue;
    const text = (next.textContent ?? "").trim();
    const m = text.match(/^\{([^{}]*)\}$/);
    if (!m) continue;

    const { htmlAttrs, classes, styles } = parseImageAttrBlock(m[1]);
    for (const [k, v] of Object.entries(htmlAttrs)) {
      if (k === "class") {
        classes.push(...String(v).split(/\s+/).filter(Boolean));
      } else if (v != null && v !== "") {
        el.setAttribute(k, v);
      }
    }
    if (classes.length) {
      const prev = (el.getAttribute("class") || "").split(/\s+/).filter(Boolean);
      el.setAttribute("class", [...new Set([...prev, ...classes])].join(" "));
    }
    if (styles.length) {
      const prev = (el.getAttribute("style") || "").trim().replace(/;?\s*$/, "");
      el.setAttribute("style", `${[prev, ...styles].filter(Boolean).join("; ")};`);
    }
    next.remove();
    changed = true;
  }

  return changed ? root.innerHTML : html;
}

/**
 * @param {string} source
 * @param {{ notePath?: string, features?: object, mermaid?: boolean, excalidraw?: boolean }} [opts]
 */
export async function renderMarkdown(source, opts = {}) {
  await yieldToMain();

  const normalized = normalizeFenceClosingAttrs(source ?? "");
  const { html } = markdownToHtml(normalized, {
    features: opts.features ?? toSatteriFeatures(),
  });

  let out = applyImageAttrs(html);
  const mermaidOn =
    opts.mermaid ?? usePreviewFeatures().features.mermaid !== false;
  if (mermaidOn) {
    await yieldToMain();
    out = await renderMermaidInHtml(out);
    out = applyTrailingBlockAttrs(out, ".mermaid-diagram, .mermaid-error");
  }

  const excalidrawOn =
    opts.excalidraw ?? usePreviewFeatures().features.excalidraw !== false;
  if (excalidrawOn && opts.notePath) {
    await yieldToMain();
    out = await renderExcalidrawInHtml(out, opts.notePath);
  }

  await yieldToMain();
  out = renderPasswdInHtml(out);

  if (opts.notePath) {
    await yieldToMain();
    out = await rewriteImgSrc(out, opts.notePath);
  }

  return out;
}

/**
 * Stable blob: URLs across preview re-renders so <img> keep the same src
 * and the browser does not reload/flash images on every markdown refresh.
 * @type {Map<string, string>}
 */
const docBlobUrlCache = new Map();

/**
 * @param {string} docPath
 * @returns {Promise<string | null>}
 */
async function blobUrlForDoc(docPath) {
  const cached = docBlobUrlCache.get(docPath);
  if (cached) return cached;
  try {
    const data = await invoke("fs_read_bytes", { name: docPath });
    const bytes = new Uint8Array(data);
    const url = URL.createObjectURL(new Blob([bytes], { type: mimeFromName(docPath) }));
    docBlobUrlCache.set(docPath, url);
    return url;
  } catch {
    return null;
  }
}

/** Drop a cached blob URL (e.g. after the file bytes change). */
export function invalidateDocBlob(docPath) {
  const url = docBlobUrlCache.get(docPath);
  if (!url) return;
  URL.revokeObjectURL(url);
  docBlobUrlCache.delete(docPath);
}

/** Revoke all cached preview image URLs. */
export function clearDocBlobCache() {
  for (const url of docBlobUrlCache.values()) URL.revokeObjectURL(url);
  docBlobUrlCache.clear();
}

/**
 * Set container HTML while reusing existing <img> nodes that share the same
 * src (stable blob: URLs). Avoids decode/flash on every preview refresh.
 * @param {HTMLElement | null | undefined} el
 * @param {string} html
 */
export function setHtmlPreservingImages(el, html) {
  if (!el) return;

  /** @type {Map<string, HTMLImageElement[]>} */
  const bySrc = new Map();
  for (const img of el.querySelectorAll("img[src]")) {
    const src = img.getAttribute("src");
    if (!src) continue;
    let list = bySrc.get(src);
    if (!list) {
      list = [];
      bySrc.set(src, list);
    }
    list.push(img);
  }

  el.innerHTML = html ?? "";

  for (const img of [...el.querySelectorAll("img[src]")]) {
    const src = img.getAttribute("src");
    if (!src) continue;
    const prev = bySrc.get(src)?.shift();
    if (!prev) continue;
    // Keep the decoded bitmap; sync attrs (e.g. width style) from fresh markup.
    for (const name of [...prev.getAttributeNames()]) {
      if (name === "src") continue;
      if (!img.hasAttribute(name)) prev.removeAttribute(name);
    }
    for (const name of img.getAttributeNames()) {
      if (name === "src") continue;
      prev.setAttribute(name, img.getAttribute(name) ?? "");
    }
    img.replaceWith(prev);
  }
}

/**
 * Replace local image src with stable blob: URLs loaded from the document root.
 * @param {string} html
 * @param {string} notePath
 */
async function rewriteImgSrc(html, notePath) {
  const re = /<img\b([^>]*?)\bsrc="([^"]+)"([^>]*)>/gi;
  const matches = [...html.matchAll(re)];
  if (!matches.length) return html;

  let out = html;

  for (const m of matches) {
    const src = m[2];
    if (/^(https?:|data:|blob:|#)/i.test(src)) continue;
    if (/\.excalidraw$/i.test(src.split(/[?#]/)[0])) continue;
    const docPath = resolveDocPath(notePath, src);
    if (!docPath) continue;

    const url = await blobUrlForDoc(docPath);
    if (!url) continue;

    out = out.replaceAll(`src="${src}"`, `src="${url}"`);
  }

  return out;
}

function mimeFromName(name) {
  const lower = name.toLowerCase();
  if (lower.endsWith(".png")) return "image/png";
  if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
  if (lower.endsWith(".gif")) return "image/gif";
  if (lower.endsWith(".webp")) return "image/webp";
  if (lower.endsWith(".svg")) return "image/svg+xml";
  return "application/octet-stream";
}

/**
 * Save a pasted image next to the note and return the markdown snippet.
 * @param {string} notePath
 * @param {File | Blob} blob
 * @param {string} [extHint]
 */
export async function savePastedImage(notePath, blob, extHint) {
  const ext = extFromBlob(blob, extHint);
  const stamp = new Date()
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(/\.\d+Z$/, "Z");
  const rand = Math.random().toString(36).slice(2, 8);
  const rel = `.assets/paste-${stamp}-${rand}.${ext}`;
  const full = `${noteDir(notePath)}${rel}`;

  const buf = new Uint8Array(await blob.arrayBuffer());
  await invoke("fs_write_bytes", { name: full, data: Array.from(buf) });
  return { path: full, markdown: `![](${rel})` };
}

function extFromBlob(blob, hint) {
  if (hint) return hint.replace(/^\./, "");
  const t = blob.type || "";
  if (t.includes("png")) return "png";
  if (t.includes("jpeg") || t.includes("jpg")) return "jpg";
  if (t.includes("gif")) return "gif";
  if (t.includes("webp")) return "webp";
  return "png";
}

export function isMarkdownFile(name) {
  return /\.(md|markdown|mdx)$/i.test(name);
}

export function isExcalidrawFile(name) {
  return /\.excalidraw$/i.test(name);
}

export function isTextEditable(name, fileType) {
  if (isMarkdownFile(name)) return true;
  if (isExcalidrawFile(name)) return true;
  if (fileType === 1 || fileType === 2) return true;
  return /\.(txt|text|json|csv|yml|yaml|toml|rs|js|ts|css|html)$/i.test(name);
}
