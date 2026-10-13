import type { LfoShape, PanLfoParams } from "@/bindings"
import { formatMs, formatPan } from "@/components/audio"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import { useTempo } from "@/lib/store"

import type { EditorProps } from "../editor-props"
import { useDisplayCanvas } from "../use-display-canvas"
import {
  cycleSeconds,
  lfoWave,
  motionGain,
  motionPan,
  panReach,
  type MotionSettings,
} from "./lfo"
import { translatedText as t, message } from "@/lib/i18n"

/** Cycles the picture shows, so a random shape has more than one level. */
const CYCLES = 2
const POINTS = 160

const COLORS = {
  grid: "var(--wf-grid-line)",
  gridStrong: "var(--wf-grid-line-strong)",
  pan: "var(--foreground)",
  volume: "var(--muted-foreground)",
} as const

/**
 * Pan and volume with a motion of their own: where the sound sits, how far
 * and in what shape it moves, at a rate or in time with the song. The
 * picture shows two cycles of the pan (solid, left at the bottom) and of
 * the volume (faint).
 */
export function PanMotionEditor({ bind }: EditorProps<PanLfoParams>) {
  const tempo = useTempo()
  const synced = bind.value("sync") >= 0.5
  const shapeInfo = bind.info("shape")
  const choice = shapeInfo.choices[bind.value("shape")] ?? shapeInfo.choices[0]
  const shape = choice.value as LfoShape
  const settings: MotionSettings = {
    pan: bind.value("pan"),
    depth: bind.value("depth"),
    gain: bind.value("gain"),
    volumeDepth: bind.value("volumeDepth"),
  }
  const seconds = cycleSeconds(
    { sync: synced, beats: bind.value("beats"), rateHz: bind.value("rateHz") },
    tempo
  )
  const [lowest, highest] = panReach(settings)

  const canvas = useDisplayCanvas(COLORS, (context, size, colors) => {
    const inset = 6
    const height = Math.max(1, size.height - 2 * inset)
    const y = (unit: number) => inset + (1 - unit) * height
    context.lineWidth = 1
    context.strokeStyle = colors.gridStrong
    context.beginPath()
    context.moveTo(0, Math.round(y(0.5)) + 0.5)
    context.lineTo(size.width, Math.round(y(0.5)) + 0.5)
    context.stroke()
    context.strokeStyle = colors.grid
    for (let cycle = 1; cycle < CYCLES; cycle += 1) {
      const x = Math.round((cycle / CYCLES) * size.width) + 0.5
      context.beginPath()
      context.moveTo(x, inset)
      context.lineTo(x, inset + height)
      context.stroke()
    }
    const trace = (value: (wave: number) => number) => {
      context.beginPath()
      for (let index = 0; index < POINTS; index += 1) {
        const along = index / (POINTS - 1)
        const wave = lfoWave(shape, Math.min(along * CYCLES, CYCLES - 1e-9))
        const x = along * size.width
        if (index === 0) context.moveTo(x, y(value(wave)))
        else context.lineTo(x, y(value(wave)))
      }
      context.stroke()
    }
    context.strokeStyle = colors.volume
    trace((wave) => Math.min(1, motionGain(settings, wave) / 2))
    context.lineWidth = 1.5
    context.strokeStyle = colors.pan
    trace((wave) => (motionPan(settings, wave) + 1) / 2)
  })

  return (
    <div
      data-slot="pan-motion-editor"
      role="group"
      aria-label={t("Pan Motion settings")}
      className="flex min-w-0 flex-col gap-2"
    >
      <div
        data-slot="motion-display"
        role="img"
        aria-label={message("Pan moves between {pan} and {pan2} as a {choiceLabel}, one cycle every {ms}", { pan: formatPan(lowest), pan2: formatPan(highest), choiceLabel: choice.label.toLowerCase(), ms: formatMs(seconds * 1000) })}
        className="relative h-16 min-w-0 overflow-hidden rounded-md border border-border/80 bg-chassis/50"
      >
        <canvas
          ref={canvas}
          aria-hidden
          className="absolute inset-0 size-full"
        />
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(9rem,1fr))] items-start gap-2">
        <ParamGroup
          title="Motion"
          aside={
            <ParamControl
              {...bind("sync")}
              layout="inline"
              label="Sync"
              description="Follow the tempo with a number of beats per cycle, or set a rate in Hz"
            />
          }
        >
          <ParamControl
            {...bind("shape")}
            label={null}
            choiceStyle="select"
            className="w-full"
            description="The shape of one cycle. Square, saw and random glide over their jumps so they do not click"
          />
          <ParamRow columns={2}>
            {synced ? (
              <ParamControl
                {...bind("beats")}
                label="Beats"
                description="Quarter notes one cycle lasts"
              />
            ) : (
              <ParamControl
                {...bind("rateHz")}
                description="Cycles per second"
              />
            )}
            <p
              data-slot="motion-cycle"
              className="self-center text-[10px] leading-snug text-muted-foreground"
            >
              <span className="block font-readout text-[11px] text-foreground">
                {formatMs(seconds * 1000)}
              </span>
              {t("per cycle")}
            </p>
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Pan">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("pan")}
              label="Center"
              description="Where the sound sits, and what the motion swings around"
            />
            <ParamControl
              {...bind("depth")}
              label="Depth"
              description="How far the motion moves the pan either way. 0 leaves a plain pan"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Volume">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("gain")}
              description="The level at the bottom of the motion"
            />
            <ParamControl
              {...bind("volumeDepth")}
              label="Depth"
              description="How far the motion turns the volume down at its top. 0 leaves a plain gain"
            />
          </ParamRow>
        </ParamGroup>
      </div>
    </div>
  )
}
