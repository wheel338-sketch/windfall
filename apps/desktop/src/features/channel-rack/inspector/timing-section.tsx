import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import type { Channel, ChannelTiming } from "@/bindings"
import { Knob, NumberField, percentUnit } from "@/components/audio"
import { ActionButton } from "@/components/action-button"
import { Button } from "@/components/ui/button"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { clamp, PPQ, MAX_PATTERN_STEPS, TICKS_PER_STEP } from "@/lib/units"

import { findChannel } from "../channel-ops"
import { useGestureValue } from "../use-gesture-value"
import { nextGatePreset } from "./gate-preset-step"
import { GATE_PRESETS, nextGate } from "./gate-presets"
import { nextGateScale } from "./gate-scale"
import { Section } from "./parts"
import { nextShiftPreset } from "./shift-preset-step"
import { SHIFT_PRESETS, nextShift } from "./shift-presets"
import { nextShiftScale } from "./shift-scale"
import { nextSwingMixPreset } from "./swing-mix-preset-step"
import { nextSwingMixScale } from "./swing-mix-scale"
import { SWING_PRESETS, nextSwing } from "./swing-presets"

export const DEFAULT_CHANNEL_TIMING: ChannelTiming = { swingMix: 1, gateTicks: 0, shiftTicks: 0 }

export function TimingSection({ channel }: { channel: Channel }) {
  useLanguageStore((state) => state.language)
  const { id } = channel
  const stored = channel.timing ?? DEFAULT_CHANNEL_TIMING
  // Each gesture reads the other controls at dispatch time so interleaved
  // changes do not overwrite the newest channel timing settings.
  const timing = () => findChannel(id)?.timing ?? DEFAULT_CHANNEL_TIMING
  const swing = useGestureValue(stored.swingMix, (value, dispatch) => dispatch({
    type: "updateChannel", id, patch: { timing: { ...timing(), swingMix: clamp(value, 0, 1) } },
  }))
  const gate = useGestureValue(stored.gateTicks, (value, dispatch) => dispatch({
    type: "updateChannel", id, patch: { timing: { ...timing(), gateTicks: clamp(Math.round(value), 0, MAX_PATTERN_STEPS * TICKS_PER_STEP) } },
  }))
  const shift = useGestureValue(stored.shiftTicks, (value, dispatch) => dispatch({
    type: "updateChannel", id, patch: { timing: { ...timing(), shiftTicks: clamp(Math.round(value), -PPQ, PPQ) } },
  }))
  const swingHint = useHint(t("Channel swing mix: how much of the global swing applies to this channel. 100% follows global swing; 0% stays straight"))
  const gateHint = useHint(t("Gate caps the played duration after swing, in ticks. Zero keeps the original duration. Stored piano notes stay unchanged"))
  const shiftHint = useHint(t("Shift moves played starts after swing, in ticks. Negative starts clamp to zero; starts reaching the pattern end do not play in that pass"))
  return (
    <Section title={t("Note timing")} aside={<ActionButton action="channel.resetTiming" variant="ghost" size="xs">{t("Reset")}</ActionButton>}>
      <div className="flex items-center justify-between gap-2" {...swingHint}>
        <span>{t("Swing mix")}</span>
        <Knob size="sm" aria-label={t("Channel swing mix")} min={0} max={1} defaultValue={1} {...percentUnit} {...swing} />
      </div>
      <div className="flex flex-wrap gap-1">
        {SWING_PRESETS.map(({ label, value: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextSwing(stored.swingMix, preset) === null}
            onClick={() => {
              if (nextSwing(timing().swingMix, preset) !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), swingMix: preset } },
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={i18nMessage("Choose the {value0} channel swing mix", { value0: t(direction) })}
            disabled={nextSwingMixPreset(stored.swingMix, direction) === null}
            onClick={() => {
              const next = nextSwingMixPreset(timing().swingMix, direction)
              if (next !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), swingMix: next } },
                })
              }
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["halve", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            disabled={nextSwingMixScale(stored.swingMix, factor) === null}
            onClick={() => {
              const next = nextSwingMixScale(timing().swingMix, factor)
              if (next !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), swingMix: next } },
                })
              }
            }}
          >
            {factor === "halve" ? t("Halve") : t("Double")}
          </Button>
        ))}
      </div>
      <div className="flex items-center justify-between gap-2" {...gateHint}>
        <span>{t("Gate")}</span>
        <NumberField size="sm" aria-label={t("Channel gate in ticks")} min={0} max={MAX_PATTERN_STEPS * TICKS_PER_STEP} step={1} defaultValue={0} unit="ticks" {...gate} />
      </div>
      <div className="flex flex-wrap gap-1">
        {GATE_PRESETS.map(({ label, ticks: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextGate(stored.gateTicks, preset) === null}
            onClick={() => {
              if (nextGate(timing().gateTicks, preset) !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), gateTicks: preset } },
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={i18nMessage("Choose the {value0} channel gate", { value0: t(direction) })}
            disabled={nextGatePreset(stored.gateTicks, direction) === null}
            onClick={() => {
              const next = nextGatePreset(timing().gateTicks, direction)
              if (next !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), gateTicks: next } },
                })
              }
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            disabled={nextGateScale(stored.gateTicks, factor) === null}
            onClick={() => {
              const next = nextGateScale(timing().gateTicks, factor)
              if (next !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), gateTicks: next } },
                })
              }
            }}
          >
            {factor === "half" ? t("Half") : t("Double")}
          </Button>
        ))}
      </div>
      <div className="flex items-center justify-between gap-2" {...shiftHint}>
        <span>{t("Shift")}</span>
        <NumberField size="sm" aria-label={t("Channel shift in ticks")} min={-PPQ} max={PPQ} step={1} defaultValue={0} unit="ticks" {...shift} />
      </div>
      <div className="flex flex-wrap gap-1">
        {SHIFT_PRESETS.map(({ label, ticks: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextShift(stored.shiftTicks, preset) === null}
            onClick={() => {
              if (nextShift(timing().shiftTicks, preset) !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), shiftTicks: preset } },
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={i18nMessage("Choose the {value0} channel shift", { value0: t(direction) })}
            disabled={nextShiftPreset(stored.shiftTicks, direction) === null}
            onClick={() => {
              const next = nextShiftPreset(timing().shiftTicks, direction)
              if (next !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), shiftTicks: next } },
                })
              }
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            disabled={nextShiftScale(stored.shiftTicks, factor) === null}
            onClick={() => {
              const next = nextShiftScale(timing().shiftTicks, factor)
              if (next !== null) {
                void dispatch({
                  type: "updateChannel", id, patch: { timing: { ...timing(), shiftTicks: next } },
                })
              }
            }}
          >
            {factor === "half" ? t("Half") : t("Double")}
          </Button>
        ))}
      </div>
      <p className="text-muted-foreground">{i18nMessage("{ppq} ticks = one quarter note. Gate 0 keeps note lengths; filtering groups does not affect timing.", { ppq: PPQ })}</p>
    </Section>
  )
}
