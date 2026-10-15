import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import type { Channel, ChannelTiming } from "@/bindings"
import { Knob, NumberField, percentUnit } from "@/components/audio"
import { ActionButton } from "@/components/action-button"
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
import {
  directionSteps,
  factorSteps,
  StepsMenus,
  WithSteps,
  type SettingSteps,
} from "./value-steps"

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
  const update = (patch: Partial<ChannelTiming>) =>
    void dispatch({
      type: "updateChannel", id, patch: { timing: { ...timing(), ...patch } },
    })
  const swingSteps: SettingSteps = {
    name: t("Swing mix"),
    groups: [
      SWING_PRESETS.map(({ label, value: preset }) => ({
        title: t(label),
        disabled: nextSwing(stored.swingMix, preset) === null,
        run: () => {
          if (nextSwing(timing().swingMix, preset) !== null) update({ swingMix: preset })
        },
      })),
      directionSteps(
        (direction) => {
          const next = nextSwingMixPreset(timing().swingMix, direction)
          if (next !== null) update({ swingMix: next })
        },
        (direction) => nextSwingMixPreset(stored.swingMix, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const next = nextSwingMixScale(timing().swingMix, factor === "half" ? "halve" : "double")
          if (next !== null) update({ swingMix: next })
        },
        (factor) => nextSwingMixScale(stored.swingMix, factor === "half" ? "halve" : "double") !== null,
        { half: t("Halve"), double: t("Double") }
      ),
    ],
  }
  const gateSteps: SettingSteps = {
    name: t("Gate"),
    groups: [
      GATE_PRESETS.map(({ label, ticks: preset }) => ({
        title: t(label),
        disabled: nextGate(stored.gateTicks, preset) === null,
        run: () => {
          if (nextGate(timing().gateTicks, preset) !== null) update({ gateTicks: preset })
        },
      })),
      directionSteps(
        (direction) => {
          const next = nextGatePreset(timing().gateTicks, direction)
          if (next !== null) update({ gateTicks: next })
        },
        (direction) => nextGatePreset(stored.gateTicks, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const next = nextGateScale(timing().gateTicks, factor)
          if (next !== null) update({ gateTicks: next })
        },
        (factor) => nextGateScale(stored.gateTicks, factor) !== null
      ),
    ],
  }
  const shiftSteps: SettingSteps = {
    name: t("Shift"),
    groups: [
      SHIFT_PRESETS.map(({ label, ticks: preset }) => ({
        title: t(label),
        disabled: nextShift(stored.shiftTicks, preset) === null,
        run: () => {
          if (nextShift(timing().shiftTicks, preset) !== null) update({ shiftTicks: preset })
        },
      })),
      directionSteps(
        (direction) => {
          const next = nextShiftPreset(timing().shiftTicks, direction)
          if (next !== null) update({ shiftTicks: next })
        },
        (direction) => nextShiftPreset(stored.shiftTicks, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const next = nextShiftScale(timing().shiftTicks, factor)
          if (next !== null) update({ shiftTicks: next })
        },
        (factor) => nextShiftScale(stored.shiftTicks, factor) !== null
      ),
    ],
  }
  return (
    <Section title={t("Note timing")} aside={<ActionButton action="channel.resetTiming" variant="ghost" size="xs">{t("Reset")}</ActionButton>}>
      <div className="flex items-center justify-between gap-2" {...swingHint}>
        <span>{t("Swing mix")}</span>
        <WithSteps steps={swingSteps}>
          <Knob size="sm" aria-label={t("Channel swing mix")} min={0} max={1} defaultValue={1} {...percentUnit} {...swing} />
        </WithSteps>
      </div>
      <div className="flex items-center justify-between gap-2" {...gateHint}>
        <span>{t("Gate")}</span>
        <WithSteps steps={gateSteps}>
          <NumberField size="sm" aria-label={t("Channel gate in ticks")} min={0} max={MAX_PATTERN_STEPS * TICKS_PER_STEP} step={1} defaultValue={0} unit="ticks" {...gate} />
        </WithSteps>
      </div>
      <div className="flex items-center justify-between gap-2" {...shiftHint}>
        <span>{t("Shift")}</span>
        <WithSteps steps={shiftSteps}>
          <NumberField size="sm" aria-label={t("Channel shift in ticks")} min={-PPQ} max={PPQ} step={1} defaultValue={0} unit="ticks" {...shift} />
        </WithSteps>
      </div>
      <StepsMenus settings={[swingSteps, gateSteps, shiftSteps]} />
      <p className="text-muted-foreground">{i18nMessage("{ppq} ticks = one quarter note. Gate 0 keeps note lengths; filtering groups does not affect timing.", { ppq: PPQ })}</p>
    </Section>
  )
}
