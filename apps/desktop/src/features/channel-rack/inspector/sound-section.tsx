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
import { Button } from "@/components/ui/button"
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

  return (
    <Section title={t("Sound")}>
      <div className="grid grid-cols-4 justify-items-center gap-1">
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
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {ROOT_KEY_PRESETS.map(({ label, rootKey: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextRootKey(source.rootKey, preset) === null}
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              if (nextRootKey(latest.rootKey, preset) !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { rootKey: preset },
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={
              direction === "previous"
                ? t("Choose the previous root key preset")
                : t("Choose the next root key preset")
            }
            disabled={nextRootKeyPreset(source.rootKey, direction) === null}
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextRootKeyPreset(latest.rootKey, direction)
              if (next === null) return
              void dispatch({
                type: "updateSampler",
                id,
                patch: { rootKey: next },
              })
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            aria-label={
              factor === "half"
                ? t("Halve root distance from C5")
                : t("Double root distance from C5")
            }
            disabled={nextRootKeyScale(source.rootKey, factor) === null}
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextRootKeyScale(latest.rootKey, factor)
              if (next !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { rootKey: next },
                })
              }
            }}
          >
            {factor === "half" ? t("Half") : t("Double")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {TUNE_PRESETS.map(({ label, semitones: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextSamplerTune(source.tune, cents, preset) === null}
            onClick={() => {
              const next = nextSamplerTune(source.tune, cents, preset)
              if (next !== null) {
                setCoarse(preset)
                void send({ tune: next })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={
              direction === "previous"
                ? t("Choose the previous sampler tune")
                : t("Choose the next sampler tune")
            }
            disabled={nextSamplerTunePreset(source.tune, coarse, direction) === null}
            onClick={() => {
              const next = nextSamplerTunePreset(storedTune(id), coarse, direction)
              if (next !== null) {
                setCoarse(next.semitones)
                void send({ tune: next.tune })
              }
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            disabled={nextSamplerTuneScale(source.tune, coarse, factor) === null}
            onClick={() => {
              const next = nextSamplerTuneScale(storedTune(id), coarse, factor)
              if (next !== null) {
                setCoarse(next.semitones)
                void send({ tune: next.tune })
              }
            }}
          >
            {factor === "half" ? t("Half") : t("Double")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {FINE_TUNE_PRESETS.map(({ label, cents: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextSamplerFine(source.tune, coarse, preset) === null}
            onClick={() => {
              const next = nextSamplerFine(storedTune(id), coarse, preset)
              if (next !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { tune: next },
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={
              direction === "previous"
                ? t("Choose the previous sampler fine tune")
                : t("Choose the next sampler fine tune")
            }
            disabled={
              nextSamplerFinePreset(source.tune, coarse, direction) === null
            }
            onClick={() => {
              const next = nextSamplerFinePreset(storedTune(id), coarse, direction)
              if (next !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { tune: next },
                })
              }
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            aria-label={
              factor === "half"
                ? t("Halve fine tune cents")
                : t("Double fine tune cents")
            }
            disabled={nextSamplerFineScale(source.tune, coarse, factor) === null}
            onClick={() => {
              const next = nextSamplerFineScale(storedTune(id), coarse, factor)
              if (next !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { tune: next },
                })
              }
            }}
          >
            {factor === "half" ? t("Halve cents") : t("Double cents")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {SAMPLE_GAIN_PRESETS.map(({ label, gain: preset }) => (
          <Button
            key={label}
            variant="outline"
            size="sm"
            disabled={nextSampleGain(source.gain, preset) === null}
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              if (nextSampleGain(latest.gain, preset) !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { gain: preset },
                })
              }
            }}
          >
            {t(label)}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="sm"
            aria-label={
              direction === "previous"
                ? t("Choose the previous sample gain")
                : t("Choose the next sample gain")
            }
            disabled={nextSampleGainPreset(source.gain, direction) === null}
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextSampleGainPreset(latest.gain, direction)
              if (next !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { gain: next },
                })
              }
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="mt-1 flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="sm"
            disabled={nextSampleGainScale(source.gain, factor) === null}
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextSampleGainScale(latest.gain, factor)
              if (next !== null) {
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { gain: next },
                })
              }
            }}
          >
            {factor === "half" ? t("Half") : t("Double")}
          </Button>
        ))}
      </div>
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
          </span>
        </div>
        <div className="mt-1 flex flex-wrap gap-1">
          {CUT_GROUP_PRESETS.map(({ label, cutGroup: preset }) => (
            <Button
              key={label}
              variant="outline"
              size="sm"
              disabled={nextCutGroup(source.cutGroup, preset) === null}
              onClick={() => {
                const latest = findChannel(id)?.source
                if (latest?.type !== "sampler") return
                if (nextCutGroup(latest.cutGroup, preset) !== null) {
                  void dispatch({
                    type: "updateSampler",
                    id,
                    patch: { cutGroup: preset },
                  })
                }
              }}
            >
              {t(label)}
            </Button>
          ))}
        </div>
        <div className="mt-1 flex flex-wrap gap-1">
          {(["previous", "next"] as const).map((direction) => (
            <Button
              key={direction}
              variant="outline"
              size="sm"
              aria-label={
                direction === "previous"
                  ? t("Choose the previous cut group preset")
                  : t("Choose the next cut group preset")
              }
              disabled={nextCutGroupPreset(source.cutGroup, direction) === null}
              onClick={() => {
                const latest = findChannel(id)?.source
                if (latest?.type !== "sampler") return
                const next = nextCutGroupPreset(latest.cutGroup, direction)
                if (next === null) return
                void dispatch({
                  type: "updateSampler",
                  id,
                  patch: { cutGroup: next },
                })
              }}
            >
              {direction === "previous" ? t("Previous") : t("Next")}
            </Button>
          ))}
        </div>
      </div>
    </Section>
  )
}
