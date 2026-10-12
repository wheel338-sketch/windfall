import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"
import { NotebookPanel } from "./panel"
import { NOTEBOOK_CSP, notebookHtmlDocument, notebookHtmlUrl, sanitizeNotebookHtml } from "./safe-html"

describe("inert HTML notebook", () => {
  it("retains formatting but removes executable, privileged and network content", () => {
    const html = sanitizeNotebookHtml(`<h2 onclick="window.__TAURI_INTERNALS__.invoke('dispatch')">Mix</h2>
      <script>parent.alert(1)</script><img src="https://evil.invalid/a" onerror="alert(1)">
      <iframe src="tauri://localhost"></iframe><svg><a href="javascript:alert(1)">x</a></svg>
      <style>@import 'https://evil.invalid/css';</style><form action="https://evil.invalid"><input></form>
      <a href="javascript:alert(1)" target="_top">Bad link</a>
      <a href="https://example.invalid/manual">Manual</a><strong style="background:url(https://evil.invalid)">Keep</strong>`)
    expect(html).toContain("<h2>Mix</h2>")
    expect(html).toContain("<strong>Keep</strong>")
    expect(html).toContain("Manual (https://example.invalid/manual)")
    expect(html).not.toMatch(/<(script|img|iframe|svg|style|form|input|a)\b|onclick|onerror|javascript:|__TAURI|\b(src|href|style)=/i)
  })
  it("handles hostile malformed markup, namespace and entity tricks without active elements", () => {
    for (const source of [
      '<math><mtext><table><mglyph><style><!--</style><img title="--><img src=x onerror=alert(1)>">',
      '<a href="&#106;avascript:alert(1)">safe text</a><object data="file:///secret">x</object>',
      '<base href="https://evil.invalid"><meta http-equiv="refresh" content="0;url=https://evil.invalid"><p id="__TAURI__">Hello</p>',
      '<template shadowrootmode="open"><script>alert(1)</script></template><p>End</p>',
    ]) {
      const result = sanitizeNotebookHtml(source)
      const template = document.createElement("template")
      template.innerHTML = result
      expect(template.content.querySelectorAll("script,img,iframe,object,embed,svg,math,style,meta,base,a,template")).toHaveLength(0)
      for (const element of template.content.querySelectorAll("*")) expect(element.attributes).toHaveLength(0)
    }
  })
  it("mounts a frame with no sandbox permissions and a deny-all resource policy", () => {
    render(<NotebookPanel notebook={{ pages: [{ title: "Rich", body: "<b>Lyrics</b>", html: true }] }} onSave={vi.fn()} />)
    const frame = screen.getByTitle("Notebook HTML preview")
    expect(frame).toHaveAttribute("sandbox", "")
    expect(frame).toHaveAttribute("referrerpolicy", "no-referrer")
    expect(frame).not.toHaveAttribute("srcdoc")
    expect(frame.getAttribute("src")).toMatch(/^data:text\/html;charset=utf-8,/)
    const source = () => decodeURIComponent(frame.getAttribute("src")!.split(",").slice(1).join(","))
    expect(source()).toContain(NOTEBOOK_CSP)
    expect(source()).toContain("<b>Lyrics</b>")
    fireEvent.change(screen.getByLabelText("Page body"), { target: { value: '<script>alert(1)</script><p>Edited</p>' } })
    expect(source()).toContain("<p>Edited</p>")
    expect(source()).not.toContain("<script>")
  })
  it("uses an opaque scheme and removes packaged IPC, filesystem and parent attacks before navigation", () => {
    for (const hostile of [
      `<script>parent.document.body.textContent='owned';window.__TAURI_INTERNALS__.invoke('project_new')</script><p>Keep</p>`,
      `<img src="http://ipc.localhost/dispatch"><iframe src="tauri://localhost/index.html"></iframe><object data="file:///C:/secret"></object><p>Keep</p>`,
      `<a href="asset://localhost/C:/secret" target="_top">Keep</a><meta http-equiv="refresh" content="0;url=https://evil.invalid"><style>@import 'https://evil.invalid';</style>`,
      `<iframe srcdoc="&lt;script&gt;fetch('https://evil.invalid')&lt;/script&gt;"></iframe><svg><foreignObject><script>top.location='https://evil.invalid'</script></foreignObject></svg><p>Keep</p>`,
    ]) {
      const url = notebookHtmlUrl(hostile)
      expect(new URL(url).origin).toBe("null")
      const delivered = decodeURIComponent(url.slice(url.indexOf(",") + 1))
      expect(delivered).toContain("Keep")
      expect(delivered).toContain("connect-src 'none'")
      expect(delivered).not.toMatch(/__TAURI|ipc\.localhost|tauri:\/\/|file:\/\/|asset:\/\/|<script|<iframe|<object|<svg|srcdoc=/)
      expect(url.length).toBeLessThan(250_000)
    }
  })
  it("bounds the sanitizer and puts CSP ahead of all project content", () => {
    const html = notebookHtmlDocument("<div>".repeat(10_000))
    expect(html.length).toBeLessThan(20_000)
    expect(html.indexOf("Content-Security-Policy")).toBeLessThan(html.indexOf("<body>"))
  })
  it("keeps malformed Unicode and a cut surrogate pair inert and encodable", () => {
    for (const source of ["\uD800<p>Visible note</p>\uDC00", "a".repeat(16_383) + "\uD83D\uDE00"]) {
      const url = notebookHtmlUrl(source)
      expect(new URL(url).origin).toBe("null")
      expect(decodeURIComponent(url.slice(url.indexOf(",") + 1))).toContain("\uFFFD")
      expect(url.length).toBeLessThan(250_000)
    }
  })
})
