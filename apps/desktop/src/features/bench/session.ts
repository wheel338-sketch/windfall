import type { FrameStats, TimeGridView } from "@/lib/canvas"

import { generateNotes } from "./generate-notes"
import { attachInteraction } from "./interaction"
import { paintLabels } from "./labels"
import { BenchModel } from "./model"
import type { BenchParams } from "./params"
import { runBenchmark, type BenchResults } from "./runner"
import { createScenarios } from "./scenarios"

declare global {
  interface Window {
    /** Set when the automated run has finished. Read by the Playwright driver. */
    __benchResults?: BenchResults
    __benchDone?: boolean
    __benchError?: string
  }
}

const HUD_INTERVAL_MS = 250

function describe(view: TimeGridView, params: BenchParams): string {
  const { info } = view.renderer
  const size = `${view.element.width}x${view.element.height}`
  return [
    `${info.kind}  ${params.notes.toLocaleString("en-US")} notes  ${size} @${window.devicePixelRatio}x`,
    info.device,
  ].join("\n")
}

/**
 * Fills the view with generated notes, then either runs the scripted
 * scenarios or hands the page to the mouse.
 */
export function startSession(
  view: TimeGridView,
  params: BenchParams,
  setHud: (text: string) => void
): void {
  const model = new BenchModel(view, generateNotes(params.notes, params.seed))
  if (params.labels) view.addOverlayPainter(paintLabels)
  const header = describe(view, params)

  if (params.auto) {
    setHud(`${header}\nstarting`)
    runBenchmark({ view, model }, createScenarios(), params, (status) =>
      setHud(`${header}\n${status}`)
    ).then(
      (results) => {
        window.__benchResults = results
        window.__benchDone = true
      },
      (error: unknown) => {
        window.__benchError =
          error instanceof Error ? error.message : String(error)
        window.__benchDone = true
        setHud(`${header}\nfailed: ${window.__benchError}`)
      }
    )
    return
  }

  attachInteraction(view, model)
  let frames = 0
  let drawMs = 0
  let last: FrameStats | null = null
  view.onFrame = (stats) => {
    frames++
    drawMs += stats.cpuMs
    last = stats
  }
  const hint =
    "wheel scroll, ctrl+wheel zoom, alt+wheel row zoom\ndrag to select, drag notes to move, space playhead"
  setHud(`${header}\n${hint}`)
  window.setInterval(() => {
    if (frames === 0) return
    const fps = (frames * 1000) / HUD_INTERVAL_MS
    const average = drawMs / frames
    const selected = model.items.batch.selectedCount
    setHud(
      [
        header,
        `${fps.toFixed(0)} draws/s  ${average.toFixed(2)} ms cpu per draw`,
        `${last?.itemsInRange ?? 0} notes in range  ${selected} selected`,
        hint,
      ].join("\n")
    )
    frames = 0
    drawMs = 0
  }, HUD_INTERVAL_MS)
}
