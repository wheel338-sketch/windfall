// Parse in an inert template, never a live document or DOMParser (which may
// fetch image resources). Reconstruct an attribute-free, HTML-only allowlist.
const ALLOWED = new Set([
  "p", "br", "h1", "h2", "h3", "h4", "strong", "b", "em", "i", "u",
  "s", "ul", "ol", "li", "blockquote", "pre", "code", "hr", "table",
  "thead", "tbody", "tr", "th", "td", "span", "div",
])
const DROP = new Set([
  "script", "style", "iframe", "object", "embed", "template", "svg", "math",
  "link", "meta", "base", "img", "audio", "video", "source", "form", "input",
  "button", "textarea", "select",
])
export const NOTEBOOK_CSP = "default-src 'none'; script-src 'none'; connect-src 'none'; img-src 'none'; media-src 'none'; font-src 'none'; style-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"

export function sanitizeNotebookHtml(source: string): string {
  const template = document.createElement("template")
  template.innerHTML = source.slice(0, 16_384)
  const output = document.createElement("div")
  let remaining = 4096
  function copy(node: Node, target: Node, depth: number) {
    if (--remaining < 0 || depth > 64) return
    if (node.nodeType === Node.TEXT_NODE) {
      target.appendChild(document.createTextNode(node.textContent ?? ""))
      return
    }
    if (!(node instanceof Element)) return
    const tag = node.localName.toLowerCase()
    if (node.namespaceURI !== "http://www.w3.org/1999/xhtml" || DROP.has(tag)) return
    const next = ALLOWED.has(tag) ? document.createElement(tag) : target
    if (next !== target) target.appendChild(next)
    for (const child of node.childNodes) copy(child, next, depth + 1)
    // Links are readable notes, never navigation or network requests.
    if (tag === "a") {
      const href = node.getAttribute("href")
      if (href && /^https?:\/\//i.test(href))
        target.appendChild(document.createTextNode(` (${href})`))
    }
  }
  for (const node of template.content.childNodes) copy(node, output, 0)
  return output.innerHTML
}

export function notebookHtmlDocument(source: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${NOTEBOOK_CSP}"></head><body>${sanitizeNotebookHtml(source)}</body></html>`
}
