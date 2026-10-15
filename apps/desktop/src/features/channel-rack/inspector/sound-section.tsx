import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useState } from "react"

import type { ChannelId, SamplerPatch } from "@/bindings"
import {
  faderTaper,
  formatGain,
  gainUnit,
  Knob,
  noteName,
  NumberField,
  parseNumber,
  semitonesUnit,
} from "@/components/audio"
import type { SamplerChannel } from "@/lib/channel-source"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { clamp, DEFAULT_KEY, MAX_GAIN } from "@/lib/units"

import { findChannel } from "../channel-ops"
import { joinTune, splitTune } from "../steps"
import { useGestureValue } from "../use-gesture-value"
import { nextCutGroupPreset } from "./cut-group-preset-step"
import { CUT_GROUP_PRESETS, nextCutGroup } from "./cut-group-presets"
import { nextSamplerFineScale } from "./fine-cents-scale"
import { nextSamplerFinePreset } from "./fine-tune-preset-step"
import { FINE_TUNE_PRESETS, nextSamplerFine } from "./fine-tune-presets"
import { Section, SwitchRow } from "./parts"
import { nextRootKeyPreset } from "./root-key-preset-step"
import { nextRootKey, ROOT_KEY_PRESETS } from "./root-key-presets"
import { nextRootKeyScale } from "./root-key-scale"
import { nextSampleGainPreset } from "./sample-gain-preset-step"
import { nextSampleGain, SAMPLE_GAIN_PRESETS } from "./sample-gain-presets"
import { nextSampleGainScale } from "./sample-gain-scale"
import { nextSamplerTunePreset } from "./tune-preset-step"
import { nextSamplerTune, TUNE_PRESETS } from "./tune-presets"
import { nextSamplerTuneScale } from "./tune-scale"
import {
  directionSteps,
  factorSteps,
  StepsMenus,
  WithSteps,
  type SettingSteps,
} from "./value-steps"

const MAX_TUNE = 48
const MAX_KEY = 127
const MAX_CUT_GROUP = 255

const NOTE_OFFSETS: Record<string, number> = {
  c: 0,
  d: 2,
  e: 4,
  f: 5,
  g: 7,
  a: 9,
  b: 11,
}

/** Reads "C5", "f#3", "Bb4" or a key number. FL's naming: key 60 is C5. */
export function parseNoteName(text: string): number | null {
  const match = /^\s*([a-g])\s*([#b]?)\s*(-?\d+)\s*$/i.exec(text)
  if (!match) return parseNumber(text)
  const accidental = match[2] === "#" ? 1 : match[2] === "" ? 0 : -1
  return (
    Number(match[3]) * 12 + NOTE_OFFSETS[match[1].toLowerCase()] + accidental
  )
}

const formatKey = (key: number) => noteName(Math.round(key))
const formatCents = (cents: number) => {
  const rounded = Math.round(cents)
  return `${rounded > 0 ? "+" : rounded < 0 ? "−" : ""}${Math.abs(rounded)} ct`
}

/** The channel's tuning as the project has it right now. */
function storedTune(id: ChannelId): number {
  const source = findChannel(id)?.source
  return source?.type === "sampler" ? source.tune : 0
}

/** Pitch, level and how the channel's notes cut each other off. */
export function SoundSection({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const { id, source } = channel
  const send = (patch: SamplerPatch) =>
    dispatch({ type: "updateSampler", id, patch })
  // Tune and Fine are two views of one number, and a tuning half way
  // between two semitones can be read as either. The semitone on show is
  // kept here, so Fine at +50 or -50 does not flip Tune to the neighbour.
  const [coarse, setCoarse] = useState(() => splitTune(source.tune).semitones)
  const { semitones, cents } = splitTune(source.tune, coarse)
  if (semitones !== coarse) setCoarse(semitones)

  const root = useGestureValue(source.rootKey, (value, sendInGesture) =>
    sendInGesture({
      type: "updateSampler",
      id,
      patch: { rootKey: clamp(Math.round(value), 0, MAX_KEY) },
    })
  )
  // Each knob reads the other half from the project at the moment it
  // sends, split around the semitone on show.
  const tune = useGestureValue(semitones, (value, sendInGesture) => {
    const { cents: kept } = splitTune(storedTune(id), coarse)
    // The knob now shows this semitone, so it is the one to keep.
    setCoarse(value)
    return sendInGesture({
      type: "updateSampler",
      id,
      patch: { tune: joinTune(value, kept, MAX_TUNE) },
    })
  })
  const fine = useGestureValue(cents, (value, sendInGesture) =>
    sendInGesture({
      type: "updateSampler",
      id,
      patch: {
        tune: joinTune(
          splitTune(storedTune(id), coarse).semitones,
          value,
          MAX_TUNE
        ),
      },
    })
  )
  const gain = useGestureValue(source.gain, (value, sendInGesture) =>
    sendInGesture({
      type: "updateSampler",
      id,
      patch: { gain: clamp(value, 0, MAX_GAIN) },
    })
  )
  const cutGroup = useGestureValue(source.cutGroup, (value, sendInGesture) =>
    sendInGesture({
      type: "updateSampler",
      id,
      patch: { cutGroup: clamp(Math.round(value), 0, MAX_CUT_GROUP) },
    })
  )

  const rootHint = useHint(
    i18nMessage("Root key: {value0}. The key that plays the sample at its own pitch. Double-click for C5", { value0: formatKey(root.value) })
  )
  const tuneHint = useHint(
    i18nMessage("Tune: {value0}. Whole semitones up or down. Double-click to reset", { value0: semitonesUnit.format(tune.value) })
  )
  const fineHint = useHint(
    i18nMessage("Fine tune: {value0}. Hundredths of a semitone. Double-click to reset", { value0: formatCents(fine.value) })
  )
  const gainHint = useHint(
    i18nMessage("Sample gain: {value0}. Level of the sample before the channel volume. Double-click for 0 dB", { value0: formatGain(gain.value) })
  )
  const cutHint = useHint(
    t("Cut group: channels with the same number stop each other, like an open and a closed hat. 0 means none")
  )

  /** The sampler as the project has it right now. */
  const latest = () => {
    const current = findChannel(id)?.source
    return current?.type === "sampler" ? current : null
  }
  const rootSteps: SettingSteps = {
    name: t("Root"),
    groups: [
      ROOT_KEY_PRESETS.map(({ label, rootKey: preset }) => ({
        title: t(label),
        disabled: nextRootKey(source.rootKey, preset) === null,
        run: () => {
          const now = latest()
          if (now && nextRootKey(now.rootKey, preset) !== null)
            void send({ rootKey: preset })
        },
      })),
      directionSteps(
        (direction) => {
          const now = latest()
          const next = now && nextRootKeyPreset(now.rootKey, direction)
          if (next !== null) void send({ rootKey: next })
        },
        (direction) => nextRootKeyPreset(source.rootKey, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const now = latest()
          const next = now && nextRootKeyScale(now.rootKey, factor)
          if (next !== null) void send({ rootKey: next })
        },
        (factor) => nextRootKeyScale(source.rootKey, factor) !== null,
        {
          half: t("Halve root distance from C5"),
          double: t("Double root distance from C5"),
        }
      ),
    ],
  }
  const tuneSteps: SettingSteps = {
    name: t("Tune"),
    groups: [
      TUNE_PRESETS.map(({ label, semitones: preset }) => ({
        title: t(label),
        disabled: nextSamplerTune(source.tune, cents, preset) === null,
        run: () => {
          const next = nextSamplerTune(source.tune, cents, preset)
          if (next !== null) {
            setCoarse(preset)
            void send({ tune: next })
          }
        },
      })),
      directionSteps(
        (direction) => {
          const next = nextSamplerTunePreset(storedTune(id), coarse, direction)
          if (next !== null) {
            setCoarse(next.semitones)
            void send({ tune: next.tune })
          }
        },
        (direction) =>
          nextSamplerTunePreset(source.tune, coarse, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const next = nextSamplerTuneScale(storedTune(id), coarse, factor)
          if (next !== null) {
            setCoarse(next.semitones)
            void send({ tune: next.tune })
          }
        },
        (factor) => nextSamplerTuneScale(source.tune, coarse, factor) !== null
      ),
    ],
  }
  const fineSteps: SettingSteps = {
    name: t("Fine"),
    groups: [
      FINE_TUNE_PRESETS.map(({ label, cents: preset }) => ({
        title: t(label),
        disabled: nextSamplerFine(source.tune, coarse, preset) === null,
        run: () => {
          const next = nextSamplerFine(storedTune(id), coarse, preset)
          if (next !== null) void send({ tune: next })
        },
      })),
      directionSteps(
        (direction) => {
          const next = nextSamplerFinePreset(storedTune(id), coarse, direction)
          if (next !== null) void send({ tune: next })
        },
        (direction) =>
          nextSamplerFinePreset(source.tune, coarse, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const next = nextSamplerFineScale(storedTune(id), coarse, factor)
          if (next !== null) void send({ tune: next })
        },
        (factor) => nextSamplerFineScale(source.tune, coarse, factor) !== null,
        { half: t("Halve cents"), double: t("Double cents") }
      ),
    ],
  }
  const gainSteps: SettingSteps = {
    name: t("Gain"),
    groups: [
      SAMPLE_GAIN_PRESETS.map(({ label, gain: preset }) => ({
        title: t(label),
        disabled: nextSampleGain(source.gain, preset) === null,
        run: () => {
          const now = latest()
          if (now && nextSampleGain(now.gain, preset) !== null)
            void send({ gain: preset })
        },
      })),
      directionSteps(
        (direction) => {
          const now = latest()
          const next = now && nextSampleGainPreset(now.gain, direction)
          if (next !== null) void send({ gain: next })
        },
        (direction) => nextSampleGainPreset(source.gain, direction) !== null
      ),
      factorSteps(
        (factor) => {
          const now = latest()
          const next = now && nextSampleGainScale(now.gain, factor)
          if (next !== null) void send({ gain: next })
        },
        (factor) => nextSampleGainScale(source.gain, factor) !== null
      ),
    ],
  }
  const cutGroupSteps: SettingSteps = {
    name: t("Cut group"),
    groups: [
      CUT_GROUP_PRESETS.map(({ label, cutGroup: preset }) => ({
        title: t(label),
        disabled: nextCutGroup(source.cutGroup, preset) === null,
        run: () => {
          const now = latest()
          if (now && nextCutGroup(now.cutGroup, preset) !== null)
            void send({ cutGroup: preset })
        },
      })),
      directionSteps(
        (direction) => {
          const now = latest()
          const next = now && nextCutGroupPreset(now.cutGroup, direction)
          if (next !== null) void send({ cutGroup: next })
        },
        (direction) => nextCutGroupPreset(source.cutGroup, direction) !== null
      ),
    ],
  }

  return (
    <Section title={t("Sound")}>
      <div className="grid grid-cols-4 justify-items-center gap-1">
        <WithSteps steps={rootSteps}>
          <Knob
            label={t("Root")}
            showValue
            min={0}
            max={MAX_KEY}
            step={1}
            defaultValue={DEFAULT_KEY}
            format={formatKey}
            parse={parseNoteName}
            {...root}
            {...rootHint}
          />
        </WithSteps>
        <WithSteps steps={tuneSteps}>
          <Knob
            label={t("Tune")}
            showValue
            bipolar
            min={-MAX_TUNE}
            max={MAX_TUNE}
            step={1}
            defaultValue={0}
            {...semitonesUnit}
            {...tune}
            {...tuneHint}
          />
        </WithSteps>
        <WithSteps steps={fineSteps}>
          <Knob
            label={t("Fine")}
            showValue
            bipolar
            min={-50}
            max={50}
            step={1}
            defaultValue={0}
            format={formatCents}
            {...fine}
            {...fineHint}
          />
        </WithSteps>
        <WithSteps steps={gainSteps}>
          <Knob
            label={t("Gain")}
            showValue
            min={0}
            max={MAX_GAIN}
            scale={faderTaper}
            defaultValue={1}
            {...gainUnit}
            {...gain}
            {...gainHint}
          />
        </WithSteps>
      </div>
      <StepsMenus
        settings={[rootSteps, tuneSteps, fineSteps, gainSteps, cutGroupSteps]}
      />
      <div className="mt-1 flex flex-col gap-1.5">
        <SwitchRow
          label={t("Reverse")}
          hint={t("Reverse: play the sample backwards")}
          checked={source.reverse}
          onCheckedChange={(reverse) => void send({ reverse })}
        />
        <SwitchRow
          label={t("Cut itself")}
          hint={t("Cut itself: a new note stops the one still ringing, so hits never overlap")}
          checked={source.cutSelf}
          onCheckedChange={(cutSelf) => void send({ cutSelf })}
        />
        <div
          className="flex h-6 items-center justify-between gap-2"
          {...cutHint}
        >
          <span className="truncate text-foreground/85">{t("Cut group")}</span>
          <span className="flex items-center gap-1.5">
            {cutGroup.value === 0 && (
              <span className="text-muted-foreground">{t("none")}</span>
            )}
            <WithSteps steps={cutGroupSteps}>
              <NumberField
                size="sm"
                aria-label={t("Cut group")}
                min={0}
                max={MAX_CUT_GROUP}
                step={1}
                defaultValue={0}
                className="w-12 justify-end font-readout"
                {...cutGroup}
              />
            </WithSteps>
          </span>
        </div>
      </div>
    </Section>
  )
}
