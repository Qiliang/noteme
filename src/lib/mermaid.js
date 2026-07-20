/** @type {import('mermaid').default | null} */
let mermaidApi = null;
let initialized = false;
let renderSeq = 0;

async function getMermaid() {
  if (!mermaidApi) {
    const mod = await import("mermaid");
    mermaidApi = mod.default;
  }
  if (!initialized) {
    mermaidApi.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      // Prevent Mermaid from injecting "Syntax error in text" SVG into <body>.
      suppressErrorRendering: true,
    });
    initialized = true;
  }
  return mermaidApi;
}

/** Remove leftover temp nodes Mermaid may leave on failed renders. */
function cleanupMermaidTemp(id) {
  document.getElementById(id)?.remove();
  document.getElementById(`d${id}`)?.remove();
}

function escapeHtml(text) {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function collectMermaidBlocks(doc) {
  /** @type {Element[]} */
  const blocks = [];
  for (const code of doc.querySelectorAll("pre > code.language-mermaid")) {
    const pre = code.parentElement;
    if (pre) blocks.push(pre);
  }
  for (const pre of doc.querySelectorAll("pre.mermaid")) {
    if (!blocks.includes(pre)) blocks.push(pre);
  }
  return blocks;
}

/**
 * Replace mermaid fenced code blocks with rendered SVG (offline).
 * @param {string} html
 */
export async function renderMermaidInHtml(html) {
  if (!html || !/language-mermaid|pre\.mermaid|class="mermaid"/i.test(html)) {
    return html;
  }

  const doc = new DOMParser().parseFromString(
    `<div id="nm-root">${html}</div>`,
    "text/html",
  );
  const root = doc.getElementById("nm-root");
  if (!root) return html;

  const blocks = collectMermaidBlocks(doc);
  if (!blocks.length) return html;

  const mermaid = await getMermaid();

  for (const pre of blocks) {
    // Yield between diagrams so editor input can keep running.
    await new Promise((resolve) => setTimeout(resolve, 0));

    const codeEl = pre.querySelector("code");
    const source = (codeEl?.textContent ?? pre.textContent ?? "").trim();
    if (!source) continue;

    const id = `nm-mermaid-${++renderSeq}`;
    try {
      const { svg } = await mermaid.render(id, source);
      cleanupMermaidTemp(id);
      const wrap = doc.createElement("div");
      wrap.className = "mermaid-diagram";
      wrap.innerHTML = svg;
      pre.replaceWith(wrap);
    } catch (err) {
      cleanupMermaidTemp(id);
      const msg = err instanceof Error ? err.message : String(err);
      const fail = doc.createElement("div");
      fail.className = "mermaid-error";
      fail.innerHTML = `<p class="mermaid-error-msg">Mermaid 渲染失败：${escapeHtml(msg)}</p><pre class="mermaid-error-source"><code>${escapeHtml(source)}</code></pre>`;
      pre.replaceWith(fail);
    }
  }

  return root.innerHTML;
}
