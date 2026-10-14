import { describe, expect, it } from "vitest"

import { webviewEngine } from "./webview-engine"

describe("webviewEngine", () => {
  it("tells the three desktop webviews apart by their user agents", () => {
    // Tauri on Ubuntu (WebKitGTK 2.50), on Windows (WebView2), on macOS (WKWebView).
    expect(webviewEngine("Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15")).toBe("webkitgtk")
    expect(webviewEngine("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)")).toBe("webkitgtk")
    expect(webviewEngine("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0")).toBe("chromium")
    expect(webviewEngine("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)")).toBe("wkwebview")
  })

  it("does not take Chromium on Linux or Android for WebKitGTK", () => {
    expect(webviewEngine("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36")).toBe("chromium")
    expect(webviewEngine("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Mobile Safari/537.36")).toBe("other")
    expect(webviewEngine("Mozilla/5.0 (jsdom)")).toBe("other")
  })
})
