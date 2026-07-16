import { markdownToHtml } from "satteri";
import { invoke } from "@tauri-apps/api/core";

/**
 * @param {string} notePath
 * @returns {string} directory prefix including trailing `/`, or `""` for root
 */
export function noteDir(notePath) {
  const i = notePath.lastIndexOf("/");
  return i >= 0 ? notePath.slice(0, i + 1) : "";
}

/**
 * Resolve a markdown image/link target against the note's directory.
 * @param {string} notePath
 * @param {string} href
 */
export function resolveYfsPath(notePath, href) {
  if (!href || /^(https?:|data:|blob:|#)/i.test(href)) return null;
  const cleaned = href.replace(/^\/+/, "");
  const base = noteDir(notePath);
  const parts = `${base}${cleaned}`.split("/");
  const out = [];
  for (const p of parts) {
    if (!p || p === ".") continue;
    if (p === "..") {
      if (out.length) out.pop();
      else return null;
    } else {
      out.push(p);
    }
  }
  return out.join("/");
}

/**
 * @param {string} source
 * @param {{ notePath?: string }} [opts]
 */
export async function renderMarkdown(source, opts = {}) {
  const { html } = markdownToHtml(source ?? "", {
    features: {
      gfm: true,
      frontmatter: true,
      math: true,
    },
  });

  if (!opts.notePath) return html;

  return rewriteImgSrc(html, opts.notePath);
}

/**
 * Replace local image src with blob: URLs loaded from yfs.
 * @param {string} html
 * @param {string} notePath
 */
async function rewriteImgSrc(html, notePath) {
  const re = /<img\b([^>]*?)\bsrc="([^"]+)"([^>]*)>/gi;
  const matches = [...html.matchAll(re)];
  if (!matches.length) return html;

  /** @type {Map<string, string>} */
  const cache = new Map();
  let out = html;

  for (const m of matches) {
    const src = m[2];
    const yfsPath = resolveYfsPath(notePath, src);
    if (!yfsPath) continue;

    let url = cache.get(yfsPath);
    if (!url) {
      try {
        const data = await invoke("yfs_read_bytes", { name: yfsPath });
        const bytes = new Uint8Array(data);
        const mime = mimeFromName(yfsPath);
        url = URL.createObjectURL(new Blob([bytes], { type: mime }));
        cache.set(yfsPath, url);
      } catch {
        continue;
      }
    }

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
  const rel = `assets/paste-${stamp}-${rand}.${ext}`;
  const full = `${noteDir(notePath)}${rel}`;

  const buf = new Uint8Array(await blob.arrayBuffer());
  await invoke("yfs_write_bytes", { name: full, data: Array.from(buf) });
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

export function isTextEditable(name, fileType) {
  if (isMarkdownFile(name)) return true;
  if (fileType === 1 || fileType === 2) return true;
  return /\.(txt|text|json|csv|yml|yaml|toml|rs|js|ts|css|html)$/i.test(name);
}
