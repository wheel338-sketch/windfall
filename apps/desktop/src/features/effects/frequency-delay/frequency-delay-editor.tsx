import { useState, type KeyboardEvent } from "react"

import type { EffectParams } from "@/bindings"
import { formatMs } from "@/components/audio"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import { cn } from "@/lib/utils"

import type { EditorProps } from "../editor-props"
import { BandPlot } from "./band-plot"
import {
  BAND_COUNT,
  bandParam,
  effectiveDelayMs,
  LANES,
  type Lane,
} from "./bands"

type FrequencyDelayParams = Extract<EffectParams, { type: "frequencyDelay" }>

type LaneTabsProps = {
  lane: Lane
  onChange(lane: Lane): void
}

/** Picks which setting of the bands the bars show. View state, not saved. */
function LaneTabs({ lane, onChange }: LaneTabsProps) {
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const at = LANES.findIndex((item) => item.id === lane)
    let next: number
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      next = (at + 1) % LANES.length
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      next = (at + LANES.length - 1) % LANES.length
    } else {
      return
    }
    event.preventDefault()
    onChange(LANES[next].id)
    event.currentTarget
      .querySelector<HTMLElement>(`[data-lane="${LANES[next].id}"]`)
      ?.focus()
  }
  return (
    <div
      role="radiogroup"
      aria-label="Band setting to draw"
      data-slot="band-lanes"
      onKeyDown={onKeyDown}
      className="inline-flex rounded-[5px] bg-(--wf-step-off)/70 p-px"
    >
      {LANES.map((item) => {
        const chosen = item.id === lane
        return (
          <button
            key={item.id}
            type="button"
            role="radio"
            aria-checked={chosen}
            tabIndex={chosen ? 0 : -1}
            data-lane={item.id}
            onClick={() => onChange(item.id)}
            className={cn(
              "rounded-[4px] px-2 py-0.5 text-[10px] leading-none text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
              chosen && "bg-background text-foreground shadow-sm"
            )}
          >
            {item.name}
          </button>
        )
      })}
    </div>
  )
}

/**
 * The frequency delay: sixteen bands, each with its own delay, level and
 * pan, drawn as bars. The lane switch picks which of the three the bars
 * show; the settings below the plot apply to all bands.
 */
export function FrequencyDelayEditor({
  bind,
}: EditorProps<FrequencyDelayParams>) {
  const [lane, setLane] = useState<Lane>("delayMs")
  const scale = bind.value("scale")
  const short = bind.value("shortRange") >= 0.5
  // The longest echo of any band that is switched on, as it really sounds.
  let longest = 0
  for (let band = 0; band < BAND_COUNT; band += 1) {
    if (bind.value(bandParam(band, "enabled")) < 0.5) continue
    const ms = effectiveDelayMs(bind.value(bandParam(band, "delayMs")), scale, short)
    longest = Math.max(longest, ms)
  }

  return (
    <div
      data-slot="frequency-delay-editor"
      role="group"
      aria-label="Frequency delay settings"
      className="flex min-w-0 flex-col gap-2"
    >
      <ParamGroup
        title="Bands"
        aside={<LaneTabs lane={lane} onChange={setLane} />}
      >
        <BandPlot lane={lane} bind={bind} />
      </ParamGroup>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(11.5rem,1fr))] items-start gap-2">
        <ParamGroup
          title="Time"
          aside={
            <span
              data-slot="frequency-delay-longest"
              className="font-readout text-[10px] text-muted-foreground"
            >
              <span className="sr-only">Longest echo: </span>
              {formatMs(longest)}
            </span>
          }
        >
          <ParamRow columns={3}>
            <ParamControl
              {...bind("scale")}
              description="Stretches every band's delay. Below zero it turns the shape over: short delays become long"
            />
            <ParamControl
              {...bind("feedback")}
              description="Share of each band's echo that is delayed again. Above zero the echoes repeat"
            />
            <ParamControl
              {...bind("shortRange")}
              label="Short"
              description="Makes every delay a tenth as long, for combing and widening instead of echoes"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Split">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("bandwidth")}
              description="Spreads the sixteen bands over more or less of the spectrum around its middle"
            />
            <ParamControl
              {...bind("split")}
              label="Slope"
              description="How sharply neighboring bands are kept apart"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Mix">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("dry")}
              description="Level of the sound as it came in"
            />
            <ParamControl
              {...bind("wet")}
              description="Level of the delayed bands"
            />
          </ParamRow>
        </ParamGroup>
      </div>
    </div>
  )
}
