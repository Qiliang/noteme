/**
 * Document-root path helpers shared by markdown preview and Excalidraw.
 */

/**
 * @param {string} notePath
 * @returns {string} directory prefix including trailing `/`, or `""` for root
 */
export function noteDir(notePath) {
  const i = (notePath ?? "").lastIndexOf("/");
  return i >= 0 ? notePath.slice(0, i + 1) : "";
}

/**
 * Normalize a markdown/local href for filesystem lookup.
 * Strips app-origin absolutization, decodes %XX, drops query/hash.
 * @param {string} href
 * @returns {string}
 */
export function normalizeDocHref(href) {
  let h = (href ?? "").trim();
  if (!h) return "";
  h = h.split(/[?#]/)[0];
  // WebView/DOMParser may absolutize against the Vite origin.
  h = h.replace(/^https?:\/\/(?:localhost|127\.0\.0\.1)(?::\d+)?\/?/i, "");
  try {
    h = decodeURIComponent(h);
  } catch {
    /* keep raw */
  }
  return h.trim();
}

/**
 * Resolve a markdown image/link target against the note's directory.
 * Leading `/` means document-root relative; otherwise note-relative.
 * @param {string} notePath
 * @param {string} href
 */
export function resolveDocPath(notePath, href) {
  const normalized = normalizeDocHref(href);
  if (!normalized || /^(https?:|data:|blob:|#)/i.test(normalized)) return null;

  const rootAbsolute = normalized.startsWith("/");
  const cleaned = normalized.replace(/^\/+/, "");
  if (!cleaned) return null;

  const base = rootAbsolute ? "" : noteDir(notePath ?? "");
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
 * Candidate doc-root paths for a href (note-relative, as-is, root-absolute).
 * @param {string} notePath
 * @param {string} href
 * @returns {string[]}
 */
export function docPathCandidates(notePath, href) {
  const normalized = normalizeDocHref(href);
  if (!normalized || /^(https?:|data:|blob:|#)/i.test(normalized)) return [];

  const hadRootSlash = normalized.startsWith("/");
  const cleaned = normalized.replace(/^\/+/, "");
  /** @type {string[]} */
  const ordered = [];
  /** @type {Set<string>} */
  const seen = new Set();
  const add = (p) => {
    if (p && !seen.has(p)) {
      seen.add(p);
      ordered.push(p);
    }
  };

  const base = noteDir(notePath ?? "");
  // Already-resolved paths from preview/open (avoid note-dir double prefix).
  if (base && cleaned.startsWith(base)) add(cleaned);

  if (hadRootSlash) {
    add(resolveDocPath(notePath, `/${cleaned}`));
    add(resolveDocPath(notePath, cleaned));
  } else {
    add(resolveDocPath(notePath, cleaned));
    add(resolveDocPath(notePath, `/${cleaned}`));
  }
  add(cleaned);

  return ordered;
}
