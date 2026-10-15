import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { formatPercent, Knob, percentUnit } from "@/components/audio"
import { Field, FieldDescription } from "@/components/ui/field"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import type { SamplerChannel } from "@/lib/channel-source"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { clamp } from "@/lib/units"

import { findChannel } from "../channel-ops"
import { useGestureValue } from "../use-gesture-value"
import { nextLoopCrossfadePreset } from "./crossfade-preset-step"
import {
  LOOP_CROSSFADE_PRESETS,
  nextLoopCrossfade,
} from "./loop-crossfade-presets"
import { nextLoopCrossfadeScale } from "./loop-crossfade-scale"
import { nextLoopLengthScale } from "./loop-length-scale"
import { nextLoopRegionPreset } from "./loop-region-preset-step"
import { LOOP_REGION_PRESETS, nextLoopRegion } from "./loop-region-presets"
import { nextLoopStartScale } from "./loop-start-scale"
import { Section } from "./parts"
import {
  directionSteps,
  factorSteps,
  StepsMenus,
  WithSteps,
  type SettingSteps,
} from "./value-steps"

/** A loop's points stay relative to the trimmed sample, in source order. */
export function LoopSection({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const { id, source } = channel
  const mode = source.loopMode ?? "off"
  const start = useGestureValue(source.loopStart ?? 0, (value, send) => {
    const current = findChannel(id)?.source
    const end = current?.type === "sampler" ? (current.loopEnd ?? 1) : 1
    return send({
      type: "updateSampler",
      id,
      patch: { loopStart: clamp(value, 0, Math.max(0, end - 0.000001)) },
    })
  })
  const end = useGestureValue(source.loopEnd ?? 1, (value, send) => {
    const current = findChannel(id)?.source
    const start = current?.type === "sampler" ? (current.loopStart ?? 0) : 0
    return send({
      type: "updateSampler",
      id,
      patch: { loopEnd: clamp(value, Math.min(1, start + 0.000001), 1) },
    })
  })
  const crossfade = useGestureValue(source.loopCrossfade ?? 0, (value, send) =>
    send({ type: "updateSampler", id, patch: { loopCrossfade: value } })
  )
  const crossfadeHint = useHint(
    t("Loop crossfade: blends the loop seam during playback. Zero keeps the original loop")
  )
  const startHint = useHint(
    t("Loop start: percentage of the trimmed sample, in source order")
  )
  const endHint = useHint(
    t("Loop end: exclusive end within the trimmed sample. Short loops hold at least one frame")
  )

  /** Sets the loop's points, if the channel is still a sampler. */
  const setRegion = (patch: { loopStart?: number; loopEnd?: number }) =>
    void dispatch({ type: "updateSampler", id, patch })
  /** The loop's points as the project has them right now. */
  const region = () => {
    const current = findChannel(id)?.source
    return current?.type === "sampler"
      ? { start: current.loopStart ?? 0, end: current.loopEnd ?? 1 }
      : null
  }
  /** The crossfade as the project has it, while the loop is on. */
  const looping = () => {
    const current = findChannel(id)?.source
    return current?.type === "sampler" && (current.loopMode ?? "off") !== "off"
      ? (current.loopCrossfade ?? 0)
      : null
  }
  const setCrossfade = (loopCrossfade: number) =>
    void dispatch({ type: "updateSampler", id, patch: { loopCrossfade } })
  const loopStart = source.loopStart ?? 0
  const loopEnd = source.loopEnd ?? 1
  const loopCrossfade = source.loopCrossfade ?? 0

  const regionSteps: SettingSteps = {
    name: t("Loop region"),
    groups: [
      LOOP_REGION_PRESETS.map((preset) => ({
        title: t(preset.label),
        disabled: nextLoopRegion(loopStart, loopEnd, preset) === null,
        run: () => {
          const now = region()
          if (now && nextLoopRegion(now.start, now.end, preset) !== null)
            setRegion({ loopStart: preset.start, loopEnd: preset.end })
        },
      })),
      directionSteps(
        (direction) => {
          const now = region()
          const next = now && nextLoopRegionPreset(now.start, now.end, direction)
          if (next) setRegion({ loopStart: next.start, loopEnd: next.end })
        },
        (direction) =>
          nextLoopRegionPreset(loopStart, loopEnd, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const now = region()
          const next = now && nextLoopLengthScale(now.start, now.end, factor)
          if (next !== null) setRegion({ loopEnd: next })
        },
        (factor) => nextLoopLengthScale(loopStart, loopEnd, factor) !== null,
        { half: t("Halve length"), double: t("Double length") }
      ),
      factorSteps(
        (factor) => {
          const now = region()
          const next = now && nextLoopStartScale(now.start, now.end, factor)
          if (next !== null) setRegion({ loopStart: next })
        },
        (factor) => nextLoopStartScale(loopStart, loopEnd, factor) !== null,
        { half: t("Halve from end"), double: t("Double from end") }
      ),
    ],
  }
  const crossfadeSteps: SettingSteps = {
    name: t("Crossfade"),
    groups: [
      LOOP_CROSSFADE_PRESETS.map(({ label, crossfade: preset }) => ({
        title: t(label),
        disabled:
          mode === "off" || nextLoopCrossfade(loopCrossfade, preset) === null,
        run: () => {
          const now = looping()
          if (now !== null && nextLoopCrossfade(now, preset) !== null)
            setCrossfade(preset)
        },
      })),
      directionSteps(
        (direction) => {
          const now = looping()
          const next = now === null ? null : nextLoopCrossfadePreset(now, direction)
          if (next !== null) setCrossfade(next)
        },
        (direction) =>
          mode !== "off" &&
          nextLoopCrossfadePreset(loopCrossfade, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const now = looping()
          const next = now === null ? null : nextLoopCrossfadeScale(now, factor)
          if (next !== null) setCrossfade(next)
        },
        (factor) =>
          mode !== "off" &&
          nextLoopCrossfadeScale(loopCrossfade, factor) !== null
      ),
    ],
  }

  return (
    <Section title={t("Loop")}>
      <Field>
        <ToggleGroup
          aria-label={t("Sample loop mode")}
          size="sm"
          variant="outline"
          spacing={0}
          value={[mode]}
          onValueChange={(values) => {
            const loopMode = values[0]
            if (
              loopMode === "off" ||
              loopMode === "forward" ||
              loopMode === "pingPong"
            )
              void dispatch({ type: "updateSampler", id, patch: { loopMode } })
          }}
        >
          <ToggleGroupItem value="off">{t("Off")}</ToggleGroupItem>
          <ToggleGroupItem value="forward">{t("Forward")}</ToggleGroupItem>
          <ToggleGroupItem value="pingPong">{t("Ping-pong")}</ToggleGroupItem>
        </ToggleGroup>
        <FieldDescription>{t("Repeats while held, including release. Points are percentages of the trimmed sample.")}</FieldDescription>
      </Field>
      <div className="flex justify-center gap-4">
        {/* The section says Loop, so the knobs need not: their full names
            did not fit under them. */}
        <WithSteps steps={regionSteps}>
          <Knob
            label={t("Start")}
            aria-label={t("Loop start")}
            showValue
            min={0}
            max={1}
            step={0.001}
            defaultValue={0}
            {...percentUnit}
            format={(value) => formatPercent(value, 1)}
            {...start}
            {...startHint}
          />
          <Knob
            label={t("End")}
            aria-label={t("Loop end")}
            showValue
            min={0}
            max={1}
            step={0.001}
            defaultValue={1}
            {...percentUnit}
            format={(value) => formatPercent(value, 1)}
            {...end}
            {...endHint}
          />
        </WithSteps>
        <WithSteps steps={crossfadeSteps}>
          <Knob
            label={t("Crossfade")}
            aria-label={t("Loop crossfade")}
            showValue
            min={0}
            max={1}
            step={0.001}
            defaultValue={0}
            disabled={mode === "off"}
            {...percentUnit}
            format={(value) => formatPercent(value, 1)}
            {...crossfade}
            {...crossfadeHint}
          />
        </WithSteps>
      </div>
      <StepsMenus
        settings={[regionSteps, crossfadeSteps]}
        disabled={mode === "off" ? [crossfadeSteps.name] : undefined}
      />
    </Section>
  )
}
