import { useState, type ReactNode } from "react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { validSelection, type Selection } from "./selection"
import {
  LIMITS,
  declick,
  fade,
  gain,
  insertSilence,
  invertPhase,
  normalizeTo,
  reduceNoise,
  stretch,
  trimSilence,
} from "./tools"
import type {
  AudioEditChannels,
  AudioEditOperation,
  AudioEditPreview,
  AudioFadeShape,
  AudioNormalizeMode,
  AudioStretchQuality,
} from "./types"
import { AdvancedTools } from "./advanced-tools"

const FADE_SHAPES: { value: AudioFadeShape; label: string }[] = [
  { value: "equalPower", label: "Equal power" },
  { value: "linear", label: "Linear" },
  { value: "sCurve", label: "S-curve" },
]
const NORMALIZE_MODES: { value: AudioNormalizeMode; label: string }[] = [
  { value: "peak", label: "Peak" },
  { value: "rms", label: "RMS" },
]
const PHASE_CHANNELS: { value: AudioEditChannels; label: string }[] = [
  { value: "both", label: "Both channels" },
  { value: "left", label: "Left only" },
  { value: "right", label: "Right only" },
]

/** A choice among a few named values, stored by its place in the list. */
export function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string
  value: T
  options: { value: T; label: string }[]
  onChange: (value: T) => void
  disabled: boolean
}) {
  const items = options.map((option, index) => ({
    value: index,
    label: option.label,
  }))
  return (
    <Select
      items={items}
      value={options.findIndex((option) => option.value === value)}
      disabled={disabled}
      onValueChange={(next: number | null) => {
        if (next !== null && options[next]) onChange(options[next].value)
      }}
    >
      <SelectTrigger aria-label={label}>
        <SelectValue placeholder="Not available" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {items.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

export function Amount({
  id,
  label,
  value,
  range,
  step,
  onChange,
  disabled,
}: {
  id: string
  label: string
  value: number
  range: { min: number; max: number }
  step: number
  onChange: (value: number) => void
  disabled: boolean
}) {
  const invalid =
    !Number.isFinite(value) || value < range.min || value > range.max
  return (
    <Field className="w-28" data-invalid={invalid}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type="number"
        min={range.min}
        max={range.max}
        step={step}
        disabled={disabled}
        aria-invalid={invalid}
        value={Number.isNaN(value) ? "" : value}
        onChange={(event) => onChange(event.target.valueAsNumber)}
      />
    </Field>
  )
}

function Tool({ children }: { children: ReactNode }) {
  return <div className="flex flex-wrap items-end gap-2">{children}</div>
}

/**
 * The cleaning and processing tools of the audio editor. Each button sends
 * one operation on the current selection; a control outside its range
 * disables its button instead of sending a request the editor would refuse.
 */
export function EditTools({
  preview,
  selection,
  disabled,
  onApply,
}: {
  preview: AudioEditPreview
  selection: Selection
  /** No valid selection, or an edit is running. */
  disabled: boolean
  onApply: (operation: AudioEditOperation) => void
}) {
  const [declickPercent, setDeclickPercent] = useState(50)
  const [reductionDb, setReductionDb] = useState(12)
  const [profile, setProfile] = useState<Selection | null>(null)
  const [thresholdDb, setThresholdDb] = useState(-60)
  const [gainDb, setGainDb] = useState(3)
  const [targetDb, setTargetDb] = useState(-1)
  const [mode, setMode] = useState<AudioNormalizeMode>("peak")
  const [shape, setShape] = useState<AudioFadeShape>("linear")
  const [channels, setChannels] = useState<AudioEditChannels>("both")
  const [stretchPercent, setStretchPercent] = useState(100)
  const [semitones, setSemitones] = useState(0)
  const [silenceMs, setSilenceMs] = useState(100)
  const [quality, setQuality] = useState<AudioStretchQuality>("standard")
  const [formants, setFormants] = useState(false)

  function tool(
    label: string,
    help: string,
    operation: AudioEditOperation | null
  ) {
    return (
      <Button
        variant="outline"
        size="sm"
        title={help}
        disabled={disabled || operation === null}
        onClick={() => {
          if (operation !== null) onApply(operation)
        }}
      >
        {label}
      </Button>
    )
  }

  const profileValid =
    profile !== null && validSelection(profile, preview.frames)
  const seconds = (frames: number) =>
    (frames / preview.sampleRate).toFixed(3)

  return (
    <>
      <div
        role="group"
        aria-label="Clean up"
        className="flex flex-col gap-2 rounded-md border p-2"
      >
        <p className="text-sm font-medium">Clean up</p>
        <Tool>
          {tool(
            "Remove DC offset",
            "Subtract each channel's average level over the selection",
            "removeDc"
          )}
        </Tool>
        <Tool>
          <Amount
            id="audio-edit-declick"
            label="De-click strength (%)"
            value={declickPercent}
            range={LIMITS.declickPercent}
            step={5}
            disabled={disabled}
            onChange={setDeclickPercent}
          />
          {tool(
            "De-click",
            "Find short clicks and crackle in the selection and repair them from the audio around them",
            declick(declickPercent)
          )}
        </Tool>
        <Tool>
          <Amount
            id="audio-edit-noise"
            label="Noise reduction (dB)"
            value={reductionDb}
            range={LIMITS.reductionDb}
            step={1}
            disabled={disabled}
            onChange={setReductionDb}
          />
          {tool(
            "Reduce noise",
            profileValid
              ? "Turn down what matches the captured noise profile"
              : "Estimate the steady noise in the selection and turn it down",
            reduceNoise(reductionDb, profileValid ? profile : null)
          )}
          <Button
            variant="outline"
            size="sm"
            title="Remember the selected range as noise only, to learn the noise from"
            disabled={disabled}
            onClick={() => setProfile({ ...selection })}
          >
            Use selection as noise profile
          </Button>
          {profile !== null && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setProfile(null)}
            >
              Clear noise profile
            </Button>
          )}
        </Tool>
        <p role="status" className="text-xs text-muted-foreground">
          {profile === null
            ? "No noise profile: the noise is estimated from the selection."
            : profileValid
              ? `Noise profile: frames ${profile.start}–${profile.end} (${seconds(profile.end - profile.start)} s).`
              : "The noise profile no longer fits this clip. Capture it again."}
        </p>
        <Tool>
          <Amount
            id="audio-edit-threshold"
            label="Silence below (dBFS)"
            value={thresholdDb}
            range={LIMITS.thresholdDb}
            step={1}
            disabled={disabled}
            onChange={setThresholdDb}
          />
          {tool(
            "Trim silence",
            "Remove the quiet start and end of the selection",
            trimSilence(thresholdDb)
          )}
        </Tool>
      </div>
      <div
        role="group"
        aria-label="Process"
        className="flex flex-col gap-2 rounded-md border p-2"
      >
        <p className="text-sm font-medium">Process</p>
        <Tool>
          <Amount
            id="audio-edit-gain"
            label="Gain (dB)"
            value={gainDb}
            range={LIMITS.gainDb}
            step={0.5}
            disabled={disabled}
            onChange={setGainDb}
          />
          {tool(
            "Apply gain",
            "Change the level of the selection. A boost that would clip is refused",
            gain(gainDb)
          )}
          <Amount
            id="audio-edit-target"
            label="Target (dBFS)"
            value={targetDb}
            range={LIMITS.targetDb}
            step={0.5}
            disabled={disabled}
            onChange={setTargetDb}
          />
          <Choice
            label="Normalize by"
            value={mode}
            options={NORMALIZE_MODES}
            disabled={disabled}
            onChange={setMode}
          />
          {tool(
            "Normalize to target",
            "Bring the selection's peak or RMS level to the target with one linked gain",
            normalizeTo(targetDb, mode)
          )}
        </Tool>
        <Tool>
          <Choice
            label="Fade shape"
            value={shape}
            options={FADE_SHAPES}
            disabled={disabled}
            onChange={setShape}
          />
          {tool(
            "Fade in with shape",
            "Fade the selection in from silence along the chosen curve",
            fade("in", shape)
          )}
          {tool(
            "Fade out with shape",
            "Fade the selection out to silence along the chosen curve",
            fade("out", shape)
          )}
        </Tool>
        <Tool>
          {tool(
            "Swap channels",
            "Exchange left and right in the selection",
            "swapChannels"
          )}
          {tool(
            "Mono sum",
            "Write the average of left and right to both channels",
            "monoSum"
          )}
          <Choice
            label="Invert phase of"
            value={channels}
            options={PHASE_CHANNELS}
            disabled={disabled}
            onChange={setChannels}
          />
          {tool(
            "Invert phase",
            "Flip the polarity of the chosen channels in the selection",
            invertPhase(channels)
          )}
        </Tool>
        <Tool>
          <Amount
            id="audio-edit-stretch"
            label="Length (%)"
            value={stretchPercent}
            range={LIMITS.stretchPercent}
            step={1}
            disabled={disabled}
            onChange={setStretchPercent}
          />
          <Amount
            id="audio-edit-semitones"
            label="Pitch (semitones)"
            value={semitones}
            range={LIMITS.semitones}
            step={1}
            disabled={disabled}
            onChange={setSemitones}
          />
          {tool(
            "Stretch and pitch",
            "Change the selection's length and pitch independently. The audio after it moves; nothing else changes",
            stretch(stretchPercent, semitones, quality, formants)
          )}
          <Choice<AudioStretchQuality> label="Stretch quality" value={quality} options={[{value:"fast",label:"Fast"},{value:"standard",label:"Standard"},{value:"high",label:"High"}]} onChange={setQuality} disabled={disabled} />
          <Field orientation="horizontal"><Checkbox id="editor-formants" checked={formants} disabled={disabled} onCheckedChange={(value) => setFormants(value === true)} /><FieldLabel htmlFor="editor-formants">Preserve formants</FieldLabel></Field>
        </Tool>
        <Tool>
          <Amount
            id="audio-edit-insert"
            label="Silence (ms)"
            value={silenceMs}
            range={{ min: 1, max: 120_000 }}
            step={10}
            disabled={disabled}
            onChange={setSilenceMs}
          />
          {tool(
            "Insert silence",
            "Insert silence before the selection's first frame",
            insertSilence(silenceMs, preview.sampleRate)
          )}
        </Tool>
      </div>
      <AdvancedTools preview={preview} selection={selection} disabled={disabled} onApply={onApply} />
    </>
  )
}
