/**
 * Markdown `[passwd:…]` / `[passwd:label:…]` markers — preview unlock widgets.
 *
 * Plaintext is never persisted across preview refreshes. After unlock it is
 * shown temporarily in the DOM only (caller schedules re-lock).
 */

/** Default seconds to show plaintext after unlock. */
export const PASSWD_REVEAL_SEC = 30;

const MARKER_RE = /\[passwd:([^\]]+)\]/g;

/**
 * @param {string} inner content inside `[passwd:…]`
 * @returns {{ label: string, cipher: string } | null}
 */
export function parsePasswdInner(inner) {
  const raw = (inner ?? "").trim();
  if (!raw) return null;
  const colon = raw.indexOf(":");
  if (colon > 0) {
    const label = raw.slice(0, colon).trim();
    const cipher = raw.slice(colon + 1).trim();
    // Named form: short label + base64 payload (payload is always lengthy).
    if (/^[\w.-]{1,64}$/.test(label) && cipher.length >= 16) {
      return { label, cipher };
    }
  }
  return { label: "", cipher: raw };
}

/**
 * @param {string} cipher
 * @param {string} [label]
 */
export function formatPasswdMarker(cipher, label = "") {
  const c = (cipher ?? "").trim();
  const l = (label ?? "").trim();
  if (!c) return "";
  return l ? `[passwd:${l}:${c}]` : `[passwd:${c}]`;
}

function escapeHtml(text) {
  return String(text)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function escapeAttr(text) {
  return escapeHtml(text).replaceAll("'", "&#39;");
}

/**
 * @param {string} plain
 */
export function plainToHtml(plain) {
  return escapeHtml(plain).replaceAll("\n", "<br>");
}

/**
 * Locked unlock-control HTML (always locked; no session plaintext cache).
 * @param {string} label
 * @param {string} cipher
 */
export function lockWidgetHtml(label, cipher) {
  const labelAttr = label ? ` data-passwd-label="${escapeAttr(label)}"` : "";
  const hint = label
    ? `加密内容（标签：${escapeHtml(label)}）`
    : "加密内容";
  return (
    `<span class="passwd-lock" data-passwd-cipher="${escapeAttr(cipher)}"${labelAttr}>` +
    `<span class="passwd-lock-hint">${hint}</span>` +
    `<input type="password" class="passwd-lock-input" placeholder="输入口令" autocomplete="off" spellcheck="false" />` +
    `<button type="button" class="passwd-lock-btn">解锁</button>` +
    `<span class="passwd-lock-err" hidden></span>` +
    `</span>`
  );
}

/**
 * Temporarily revealed plaintext HTML (for DOM swap after unlock).
 * @param {string} plain
 * @param {string} cipher
 * @param {string} [label]
 * @param {number} [revealSec]
 */
export function plainWidgetHtml(plain, cipher, label = "", revealSec = PASSWD_REVEAL_SEC) {
  const labelAttr = label ? ` data-passwd-label="${escapeAttr(label)}"` : "";
  const tip = `将在 ${revealSec} 秒后或刷新预览后重新锁定`;
  return (
    `<span class="passwd-plain" data-passwd-cipher="${escapeAttr(cipher)}"${labelAttr} title="${escapeAttr(tip)}">` +
    `${plainToHtml(plain)}` +
    `<span class="passwd-plain-meta">${escapeHtml(String(revealSec))}s</span>` +
    `</span>`
  );
}

/**
 * Replace `[passwd:…]` markers in rendered HTML with unlock widgets.
 * @param {string} html
 */
export function renderPasswdInHtml(html) {
  if (!html || !html.includes("[passwd:")) return html;

  // Work on text outside tags by parsing via DOM when available.
  if (typeof DOMParser === "undefined") {
    return html.replace(MARKER_RE, (_, inner) => {
      const parsed = parsePasswdInner(inner);
      if (!parsed?.cipher) return _;
      return lockWidgetHtml(parsed.label, parsed.cipher);
    });
  }

  const doc = new DOMParser().parseFromString(
    `<div id="nm-root">${html}</div>`,
    "text/html",
  );
  const root = doc.getElementById("nm-root");
  if (!root) return html;

  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  /** @type {Text[]} */
  const nodes = [];
  let n;
  while ((n = walker.nextNode())) {
    if (n.nodeValue && n.nodeValue.includes("[passwd:")) nodes.push(/** @type {Text} */ (n));
  }

  for (const textNode of nodes) {
    const text = textNode.nodeValue ?? "";
    MARKER_RE.lastIndex = 0;
    if (!MARKER_RE.test(text)) continue;
    MARKER_RE.lastIndex = 0;

    const frag = doc.createDocumentFragment();
    let last = 0;
    let m;
    while ((m = MARKER_RE.exec(text)) !== null) {
      if (m.index > last) {
        frag.appendChild(doc.createTextNode(text.slice(last, m.index)));
      }
      const parsed = parsePasswdInner(m[1]);
      if (parsed?.cipher) {
        const wrap = doc.createElement("span");
        wrap.innerHTML = lockWidgetHtml(parsed.label, parsed.cipher);
        while (wrap.firstChild) frag.appendChild(wrap.firstChild);
      } else {
        frag.appendChild(doc.createTextNode(m[0]));
      }
      last = m.index + m[0].length;
    }
    if (last < text.length) {
      frag.appendChild(doc.createTextNode(text.slice(last)));
    }
    textNode.parentNode?.replaceChild(frag, textNode);
  }

  return root.innerHTML;
}
