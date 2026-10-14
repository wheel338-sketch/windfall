import { lazy, StrictMode, Suspense } from "react"
import { createRoot } from "react-dom/client"

import "./index.css"
import App from "./App.tsx"
import { ThemeProvider } from "@/components/theme-provider.tsx"
import { useApplicationScale } from "@/lib/ui-scale-root"
import { markWebviewEngine } from "@/lib/webview-engine"

// `?view=kit` opens the audio control kit showcase instead of the app, and
// `?view=params` the editors made from the effect and instrument descriptors.
const view = new URLSearchParams(window.location.search).get("view")
const KitDemo = lazy(() => import("@/features/kit-demo"))
const ParamDemo = lazy(() => import("@/features/params/demo"))
const Showcase = view === "kit" ? KitDemo : view === "params" ? ParamDemo : null

// The stylesheet locks scrolling and text selection for the app window only.
document.documentElement.dataset.view = Showcase ? "kit" : "app"
markWebviewEngine()

function ScaledApplication() {
  useApplicationScale()
  return Showcase ? (
    <Suspense>
      <Showcase />
    </Suspense>
  ) : (
    <App />
  )
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <ScaledApplication />
    </ThemeProvider>
  </StrictMode>
)
