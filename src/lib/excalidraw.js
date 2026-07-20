import { invoke } from "@tauri-apps/api/core";
import { docPathCandidates, noteDir, resolveDocPath } from "./docPath.js";

/** @type {typeof import('@excalidraw/excalidraw') | null} */
let excalidrawApi = null;

async function getExcalidrawApi() {
  if (!excalidrawApi) {
    excalidrawApi = await import("@excalidraw/excalidraw");
  }
  return excalidrawApi;
}

function escapeHtml(text) {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

/** @param {string} src */
export function isExcalidrawSrc(src) {
  if (!src) return false;
  const path = src.split(/[?#]/)[0];
  return /\.excalidraw$/i.test(path);
}

/** Minimal empty scene matching the `.excalidraw` JSON schema. */
export function emptyExcalidrawJson() {
  return JSON.stringify(
    {
      type: "excalidraw",
      version: 2,
      source: "https://noteme.app",
      elements: [],
      appState: {
        gridSize: null,
        viewBackgroundColor: "#ffffff",
      },
      files: {},
    },
    null,
    2,
  );
}

/**
 * Create a new `.excalidraw` asset next to the note and return image-like markdown.
 * @param {string} notePath
 */
export async function createExcalidrawAsset(notePath) {
  const stamp = new Date()
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(/\.\d+Z$/, "Z");
  const rand = Math.random().toString(36).slice(2, 8);
  const rel = `.assets/excalidraw-${stamp}-${rand}.excalidraw`;
  const full = `${noteDir(notePath)}${rel}`;

  await invoke("fs_write", { name: full, content: emptyExcalidrawJson() });
  const markdown = `![Excalidraw](${rel})`;
  return { path: full, rel, markdown };
}

/**
 * Read a doc-root file, trying path candidates and short retries (Documents/iCloud races).
 * @param {string} notePath
 * @param {string} href
 * @returns {Promise<{ path: string, text: string }>}
 */
export async function readExcalidrawDoc(notePath, href) {
  const candidates = docPathCandidates(notePath, href);
  if (!candidates.length) {
    throw new Error(`无效路径 ${href || ""}`);
  }

  /** @type {unknown} */
  let lastErr = null;
  for (let attempt = 0; attempt < 4; attempt++) {
    for (const path of candidates) {
      try {
        const text = await invoke("fs_read", { name: path });
        return { path, text };
      } catch (err) {
        lastErr = err;
      }
    }
    await new Promise((r) => setTimeout(r, 40 * (attempt + 1)));
  }

  const msg =
    lastErr instanceof Error
      ? lastErr.message
      : typeof lastErr === "string"
        ? lastErr
        : String(lastErr ?? "读取失败");
  throw new Error(`${msg}（尝试: ${candidates.join(" | ")}）`);
}

/**
 * @param {Document} doc
 * @param {string} relPath
 * @param {string} notePath
 * @param {{ style?: string, className?: string, id?: string, title?: string, alt?: string }} [meta]
 */
async function renderOneDiagram(doc, relPath, notePath, meta = {}) {
  const fail = (msg) => {
    const el = doc.createElement("div");
    el.className = "excalidraw-error";
    el.innerHTML = `<p class="excalidraw-error-msg">${msg}</p>`;
    return el;
  };

  if (!relPath) {
    return fail("Excalidraw：未指定文件路径");
  }

  const resolved = resolveDocPath(notePath, relPath);
  if (!resolved && !docPathCandidates(notePath, relPath).length) {
    return fail(`Excalidraw：无效路径 ${escapeHtml(relPath)}`);
  }

  try {
    const { restore, exportToSvg } = await getExcalidrawApi();
    const { path: docPath, text } = await readExcalidrawDoc(notePath, relPath);
    const data = JSON.parse(text);
    const restored = restore(data, null, null);
    const svg = await exportToSvg({
      elements: restored.elements,
      appState: {
        ...restored.appState,
        exportBackground: true,
        viewBackgroundColor:
          restored.appState?.viewBackgroundColor || "#ffffff",
      },
      files: restored.files ?? data.files ?? null,
    });
    const wrap = doc.createElement("div");
    wrap.className = ["excalidraw-diagram", meta.className]
      .filter(Boolean)
      .join(" ");
    // Store the note-relative href so re-open does not double-prefix the note dir.
    wrap.setAttribute("data-excalidraw-path", relPath);
    wrap.setAttribute("data-excalidraw-resolved", docPath);
    wrap.setAttribute("title", meta.title || meta.alt || "⌘/Ctrl + 点击编辑");
    if (meta.id) wrap.id = meta.id;
    if (meta.style) wrap.setAttribute("style", meta.style);
    wrap.appendChild(doc.importNode(svg, true));
    return wrap;
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return fail(
      `Excalidraw 渲染失败（${escapeHtml(relPath)}）：${escapeHtml(msg)}`,
    );
  }
}

/**
 * Replace image-like `![…](*.excalidraw)` and legacy ```excalidraw fences with SVG.
 * Image attrs from `{width=300px}` (already on <img>) are copied onto the wrapper.
 * @param {string} html
 * @param {string} notePath
 */
export async function renderExcalidrawInHtml(html, notePath) {
  if (!html || !notePath) return html;
  const hasImg = /<img\b[^>]*\.excalidraw/i.test(html);
  const hasFence = /language-excalidraw/i.test(html);
  if (!hasImg && !hasFence) return html;

  const doc = new DOMParser().parseFromString(
    `<div id="nm-root">${html}</div>`,
    "text/html",
  );
  const root = doc.getElementById("nm-root");
  if (!root) return html;

  for (const img of [...root.querySelectorAll("img[src]")]) {
    const src = img.getAttribute("src") || "";
    if (!isExcalidrawSrc(src)) continue;
    await new Promise((resolve) => setTimeout(resolve, 0));
    const wrap = await renderOneDiagram(doc, src, notePath, {
      style: img.getAttribute("style") || "",
      className: img.getAttribute("class") || "",
      id: img.getAttribute("id") || "",
      title: img.getAttribute("title") || "",
      alt: img.getAttribute("alt") || "",
    });
    img.replaceWith(wrap);
  }

  for (const code of [...root.querySelectorAll("pre > code.language-excalidraw")]) {
    const pre = code.parentElement;
    if (!pre) continue;
    await new Promise((resolve) => setTimeout(resolve, 0));
    const relPath = (code.textContent ?? "").trim();
    const wrap = await renderOneDiagram(doc, relPath, notePath);
    pre.replaceWith(wrap);
  }

  return root.innerHTML;
}
