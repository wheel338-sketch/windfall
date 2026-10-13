import { memo, type PointerEvent, type Ref } from "react"

import type { TimeSignature, Timeline } from "@/bindings"
import { useHint } from "@/lib/store/hint"
import { seek, useTransportStore } from "@/lib/store/transport"
import { clamp, TICKS_PER_STEP } from "@/lib/units"
import { cn } from "@/lib/utils"

import { pitches, STEP_GAP } from "./layout"
import { rulerMarks } from "./steps"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

type StepRulerProps = {
  lengthSteps: number
  signature: TimeSignature
  timeline?: Timeline
  /** The playhead mark. The rack moves it without rendering. */
  caretRef: Ref<HTMLDivElement>
}

/**
 * Bar and beat numbers over the step columns. Every beat gets a mark at the
 * left edge of its first step; a bar line shows the bar number, brighter.
 * Clicking the ruler moves the playhead to that step.
 */
export const StepRuler = memo(function StepRuler({
  lengthSteps,
  signature,
  timeline,
  caretRef,
}: StepRulerProps) {
  const marks = rulerMarks(lengthSteps, signature, timeline)
  const hint = useHint(
    t("Bars and beats of the pattern. Click to move the playhead there")
  )

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || useTransportStore.getState().mode !== "pattern") return
    const { left, width } = event.currentTarget.getBoundingClientRect()
    const step = clamp(
      Math.floor(((event.clientX - left) / width) * lengthSteps),
      0,
      lengthSteps - 1
    )
    void seek(step * TICKS_PER_STEP)
  }

  return (
    <div
      role="img"
      aria-label={message("Ruler: {lengthSteps} steps", { lengthSteps: plainNumber(lengthSteps) })}
      className="relative h-full shrink-0 cursor-pointer overflow-hidden"
      style={{ width: pitches(lengthSteps) }}
      onPointerDown={onPointerDown}
      {...hint}
    >
      {marks.map((mark) => (
        <span
          key={mark.step}
          aria-hidden
          className={cn(
            "absolute bottom-0 flex h-full items-end border-l pb-[5px] pl-1 font-readout text-[0.625rem] leading-none",
            mark.bar
              ? "border-(--wf-grid-line-strong) text-foreground"
              : "h-1/2 border-(--wf-grid-line) text-muted-foreground/80"
          )}
          style={{ left: pitches(mark.step) }}
        >
          {mark.label}
        </span>
      ))}
      <div
        ref={caretRef}
        aria-hidden
        data-slot="rack-playhead"
        className="pointer-events-none invisible absolute bottom-0 left-0 h-[3px] rounded-t-[1px] bg-(--wf-playhead)"
        style={{ width: pitches(1, -STEP_GAP) }}
      />
    </div>
  )
})
