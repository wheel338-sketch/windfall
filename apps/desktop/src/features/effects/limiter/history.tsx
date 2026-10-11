import { useRef } from "react"

import {
  AnalyzerCanvas,
  type AnalyzerPainter,
} from "@/features/analyzers/analyzer-canvas"
import { dbPosition, History, levelToDb } from "@/features/analyzers/math"
import type { AnalyzerFrame } from "@/features/analyzers/types"
import { cn } from "@/lib/utils"

/** Blocks the display keeps: about 2.7 seconds at 48 kHz. */
export const HISTORY_BLOCKS = 512
/** The quietest level the display shows, in dB. The loudest is 0. */
export const HISTORY_FLOOR_DB = -48
/** A gate that has shut this far is drawn as shut. */
const GATE_SHUT_DB = 6

// Input level, output level, reduction and gate, each in dB.
const INPUT = 0
const OUTPUT = 1
const REDUCTION = 2
const GATE = 3

type HistoryProps = {
  /** The analyzer slot bound to the limiter. */
  slot: string
  className?: string
}

/**
 * The limiter at work, scrolling right to left: the level that came in as a
 * filled shape, the level that went out as a line over it, and the gain the
 * limiter applied as a line hanging from the top. Every column is one block
 * of audio as the limiter itself measured it. Where blocks are missing, the
 * shapes break instead of being joined across the hole.
 */
export function LimiterHistory({ slot, className }: HistoryProps) {
  const root = useRef<HTMLDivElement>(null)
  const blocks = useRef<History | null>(null)
  const count = useRef(0)

  const rows = () => (blocks.current ??= new History(HISTORY_BLOCKS, 4))

  const onFrame = (frame: AnalyzerFrame) => {
    const history = rows()
    // A break in the audio is a column of nothing.
    if (frame.gap && history.length > 0) history.push([])
    let deepest = 0
    for (const observation of frame.observations) {
      if (observation.type !== "dynamics") continue
      history.push([
        levelToDb(observation.inputPeak),
        levelToDb(observation.outputPeak),
        observation.reductionDb,
        observation.gateDb,
      ])
      deepest = Math.max(deepest, observation.reductionDb)
      count.current += 1
    }
    const element = root.current
    if (element) {
      element.dataset.blocks = String(count.current)
      element.dataset.reductionDb = deepest.toFixed(2)
    }
  }

  const onClear = () => {
    blocks.current?.clear()
    count.current = 0
    const element = root.current
    if (element) {
      element.dataset.blocks = "0"
      delete element.dataset.reductionDb
    }
  }

  const paint: AnalyzerPainter = (context, size, colors) => {
    const { width, height } = size
    const history = rows()
    const y = (db: number) => (1 - dbPosition(db, HISTORY_FLOOR_DB)) * height
    const step = width / HISTORY_BLOCKS
    const x = (entry: number) => width - (history.length - entry) * step

    context.lineWidth = 1
    context.strokeStyle = colors.grid
    context.beginPath()
    for (let db = -12; db > HISTORY_FLOOR_DB; db -= 12) {
      const line = Math.round(y(db)) + 0.5
      context.moveTo(0, line)
      context.lineTo(width, line)
    }
    context.stroke()

    // The gate: a strip along the bottom wherever it was shut.
    context.fillStyle = colors.mid
    for (let entry = 0; entry < history.length; entry += 1) {
      if (history.at(entry, GATE) >= GATE_SHUT_DB) {
        context.fillRect(x(entry), height - 3, Math.max(1, step), 3)
      }
    }

    // What came in, filled from the floor.
    context.fillStyle = colors.second
    context.globalAlpha = 0.35
    for (let entry = 0; entry < history.length; entry += 1) {
      const level = history.at(entry, INPUT)
      if (Number.isNaN(level)) continue
      const top = y(level)
      context.fillRect(x(entry), top, Math.max(1, step), height - top)
    }
    context.globalAlpha = 1

    // What went out, and the gain that was applied, as lines that break
    // where a block is missing.
    const trace = (index: number, color: string, toY: (value: number) => number) => {
      context.strokeStyle = color
      context.beginPath()
      let drawing = false
      for (let entry = 0; entry < history.length; entry += 1) {
        const value = history.at(entry, index)
        if (Number.isNaN(value)) {
          drawing = false
          continue
        }
        const at = x(entry) + step / 2
        if (drawing) context.lineTo(at, toY(value))
        else context.moveTo(at, toY(value))
        drawing = true
      }
      context.stroke()
    }
    trace(OUTPUT, colors.low, y)
    trace(REDUCTION, colors.high, (reduction) => y(-reduction))
  }

  return (
    <div
      ref={root}
      data-slot="limiter-history"
      data-blocks="0"
      className={cn(
        "relative h-20 overflow-hidden rounded-[5px] bg-(--wf-meter-bg) ring-1 ring-(--wf-grid-line-strong)",
        className
      )}
    >
      <AnalyzerCanvas
        slot={slot}
        label="Limiter level history: input level, output level and gain reduction over the last seconds"
        paint={paint}
        onFrame={onFrame}
        onClear={onClear}
        className="absolute inset-0"
      />
    </div>
  )
}
