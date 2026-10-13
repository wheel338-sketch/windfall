import { createRoot } from "react-dom/client"

import "@/index.css"

import { BenchApp } from "./BenchApp"
import { parseParams } from "./params"
import { translatedText as t } from "@/lib/i18n"

const params = parseParams(window.location.search)

// The canvas reads its colors from the theme's CSS variables, so the class
// has to be in place before the view is created.
document.documentElement.classList.remove("light", "dark")
document.documentElement.classList.add(params.theme)

const root = document.getElementById("root")
if (!root) throw new Error(t("bench.html has no #root element"))

// No StrictMode: its double mount would create and destroy a GPU context
// before the measured one.
createRoot(root).render(<BenchApp params={params} />)
