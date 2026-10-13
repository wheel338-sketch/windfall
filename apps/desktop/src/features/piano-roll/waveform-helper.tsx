import { create } from "zustand"
import type { Lane, SampleId, SampleInfo } from "@/bindings"
import { Button } from "@/components/ui/button"
import { NumberField } from "@/components/audio/number-field"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { useSampleInfo } from "@/features/channel-rack/inspector/sample-info"
import { indexBatch, keyToRow, RectBatch, RECT_FLAT, withAlpha, type GridTheme, type IndexedBatch } from "@/lib/canvas"
import { useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { MAX_PATTERN_STEPS, MAX_SONG_TICKS, PPQ, TICKS_PER_STEP } from "@/lib/units"
import { nextWaveformCenterScale } from "./waveform-center-scale"
import { nextWaveformFit } from "./waveform-fit-step"
import { nextWaveformHeightScale } from "./waveform-height-scale"
import { nextWaveformLengthScale } from "./waveform-length-scale"
import { nextWaveformOpacityScale } from "./waveform-opacity-scale"
import { nextWaveformStartScale } from "./waveform-start-scale"
import { translatedText as t } from "@/lib/i18n"

type Fit = "seconds" | "pattern" | "custom"
type HelperSettings = { sample: SampleId | null; visible: boolean; fit: Fit; start: number; length: number; anchorKey: number; height: number; opacity: number }
type HelperState = HelperSettings & { open: boolean; configure(patch: Partial<HelperSettings>): void; setOpen(open: boolean): void }
const initial: HelperSettings = { sample: null, visible: false, fit: "seconds", start: 0, length: PPQ * 4, anchorKey: 60, height: 24, opacity: 0.18 }
export const useWaveformHelper = create<HelperState>((set) => ({ ...initial, open: false, configure: (patch) => set(patch), setOpen: (open) => set({ open }) }))
onProjectReplaced(() => useWaveformHelper.setState({ ...initial, open: false }))

export function openWaveformHelper() {
  const state = useWaveformHelper.getState()
  if (state.sample === null) {
    const { project } = useProjectStore.getState()
    const channel = project.channels.find((channel) => channel.id === useUiStore.getState().selectedChannel)
    const sample = channel?.source.type === "sampler" ? channel.source.sample : null
    state.configure({ sample: sample ?? project.samples[0]?.id ?? null })
  }
  state.setOpen(true)
}

/** A reference waveform and ghosts share the ordinary renderer underlay. */
export function helperDurationTicks(helper: HelperSettings, info: SampleInfo, tempo: number, patternTicks: number): number {
  return Math.max(1, helper.fit === "pattern" ? patternTicks : helper.fit === "custom" ? helper.length
    : Math.min(MAX_SONG_TICKS, Math.round(info.durationSecs * tempo * PPQ / 60)))
}

export function buildPianoUnderlay(lanes: readonly Lane[], theme: GridTheme, helper: HelperSettings, info: SampleInfo | null, tempo: number, patternTicks: number): IndexedBatch | null {
  const wave = helper.visible && info && info.peaks.length >= 2 && info.durationSecs > 0 ? info : null
  const duration = wave ? helperDurationTicks(helper, wave, tempo, patternTicks) : 0
  const columns = wave ? Math.min(2048, Math.floor(wave.peaks.length / 2), Math.max(1, duration)) : 0
  const count = lanes.reduce((count, lane) => count + lane.notes.length, columns)
  if (count === 0) return null
  const batch = new RectBatch(count)
  if (wave) {
    const buckets = Math.floor(wave.peaks.length / 2)
    const center = keyToRow(helper.anchorKey) + 0.5
    const color = withAlpha(theme.mutedForeground, helper.opacity)
    for (let column = 0; column < columns; column++) {
      const first = Math.floor(column * buckets / columns)
      const last = Math.max(first + 1, Math.floor((column + 1) * buckets / columns))
      let min = 0; let max = 0
      for (let bucket = first; bucket < last; bucket++) {
        min = Math.min(min, wave.peaks[bucket * 2]); max = Math.max(max, wave.peaks[bucket * 2 + 1])
      }
      const top = Math.max(0, Math.floor(center - Math.min(1, max) * helper.height / 2))
      const bottom = Math.min(128, Math.ceil(center - Math.max(-1, min) * helper.height / 2))
      if (bottom <= top) continue
      const start = helper.start + Math.floor(column * duration / columns)
      const end = helper.start + Math.floor((column + 1) * duration / columns)
      batch.push(-1_000_000 - column, start, Math.max(1, end - start), top, bottom - top, color, RECT_FLAT)
    }
  }
  const ghostColor = withAlpha(theme.mutedForeground, 0.3)
  for (const lane of lanes) for (const note of lane.notes) {
    batch.push(note.id, note.start, note.length, keyToRow(note.key), 1, ghostColor)
  }
  return batch.count ? indexBatch(batch) : null
}

export function WaveformHelperControl() {
  const helper = useWaveformHelper()
  const samples = useProjectStore((state) => state.project.samples)
  const asset = samples.find((sample) => sample.id === helper.sample)
  const loaded = useSampleInfo(helper.open || helper.visible ? asset : undefined)
  const maxTicks = MAX_PATTERN_STEPS * TICKS_PER_STEP
  return <Popover open={helper.open} onOpenChange={helper.setOpen}>
    <PopoverTrigger render={<Button size="sm" variant={helper.visible ? "secondary" : "outline"} onClick={() => { if (!helper.open) openWaveformHelper() }} />}>{t("Waveform")}</PopoverTrigger>
    <PopoverContent align="start" className="w-80">
      <FieldGroup>
        <Field><FieldLabel>{t("Waveform timing reference")}</FieldLabel>
          <Select items={[{ value: "none", label: t("Choose project audio") }, ...samples.map((sample) => ({ value: String(sample.id), label: sample.name }))]} value={helper.sample === null ? "none" : String(helper.sample)} onValueChange={(value) => {
            const selected = samples.find((sample) => String(sample.id) === value)
            helper.configure({ sample: selected?.id ?? null, visible: !!selected })
          }}>
            <SelectTrigger aria-label={t("Waveform helper audio")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>
              <SelectItem value="none">{t("Choose project audio")}</SelectItem>{samples.map((sample) => <SelectItem key={sample.id} value={String(sample.id)}>{sample.name}</SelectItem>)}
            </SelectGroup></SelectContent>
          </Select>
          <FieldDescription>{!samples.length ? t("Add audio to a channel or playlist to use its waveform here.") : t("Show project audio behind the notes while editing their timing.")}</FieldDescription>
        </Field>
        <Button type="button" size="sm" variant={helper.visible ? "secondary" : "outline"} disabled={!asset} aria-pressed={helper.visible} onClick={() => helper.configure({ visible: !helper.visible })}>{helper.visible ? t("Hide waveform") : t("Show waveform")}</Button>
        <Field><FieldLabel>{t("Time mapping")}</FieldLabel><Select value={helper.fit} onValueChange={(fit) => { if (fit === "seconds" || fit === "pattern" || fit === "custom") helper.configure({ fit }) }}>
          <SelectTrigger aria-label={t("Waveform helper time mapping")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>
            <SelectItem value="seconds">{t("Original duration at project tempo")}</SelectItem><SelectItem value="pattern">{t("Fit to pattern")}</SelectItem><SelectItem value="custom">{t("Custom length")}</SelectItem>
          </SelectGroup></SelectContent>
        </Select>
          <div className="flex gap-1">
            <Button type="button" variant="outline" size="sm" aria-label={t("Choose the previous waveform time mapping")} disabled={nextWaveformFit(helper.fit, "previous") === null} onClick={() => {
              const latest = useWaveformHelper.getState()
              const next = nextWaveformFit(latest.fit, "previous")
              if (next !== null) latest.configure({ fit: next })
            }}>{t("Previous")}</Button>
            <Button type="button" variant="outline" size="sm" aria-label={t("Choose the next waveform time mapping")} disabled={nextWaveformFit(helper.fit, "next") === null} onClick={() => {
              const latest = useWaveformHelper.getState()
              const next = nextWaveformFit(latest.fit, "next")
              if (next !== null) latest.configure({ fit: next })
            }}>{t("Next")}</Button>
          </div>
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field><FieldLabel>{t("Start (ticks)")}</FieldLabel><NumberField value={helper.start} min={-maxTicks} max={maxTicks} step={1} coarseStep={60} aria-label={t("Waveform helper start")} onValueChange={(start) => helper.configure({ start: Math.round(start) })} />
            <div className="flex gap-1">
              <Button type="button" variant="outline" size="sm" aria-label={t("Halve waveform start")} disabled={nextWaveformStartScale(helper.start, "half") === null} onClick={() => {
                const latest = useWaveformHelper.getState()
                const next = nextWaveformStartScale(latest.start, "half")
                if (next !== null) latest.configure({ start: next })
              }}>{t("Half")}</Button>
              <Button type="button" variant="outline" size="sm" aria-label={t("Double waveform start")} disabled={nextWaveformStartScale(helper.start, "double") === null} onClick={() => {
                const latest = useWaveformHelper.getState()
                const next = nextWaveformStartScale(latest.start, "double")
                if (next !== null) latest.configure({ start: next })
              }}>{t("Double")}</Button>
            </div>
          </Field>
          {helper.fit === "custom" && <Field><FieldLabel>{t("Length (ticks)")}</FieldLabel><NumberField value={helper.length} min={1} max={MAX_SONG_TICKS} step={1} coarseStep={240} aria-label={t("Waveform helper length")} onValueChange={(length) => helper.configure({ length: Math.round(length) })} />
            <div className="flex gap-1">
              <Button type="button" variant="outline" size="sm" aria-label={t("Halve waveform length")} disabled={nextWaveformLengthScale(helper.length, "half") === null} onClick={() => {
                const latest = useWaveformHelper.getState()
                if (latest.fit !== "custom") return
                const next = nextWaveformLengthScale(latest.length, "half")
                if (next !== null) latest.configure({ length: next })
              }}>{t("Half")}</Button>
              <Button type="button" variant="outline" size="sm" aria-label={t("Double waveform length")} disabled={nextWaveformLengthScale(helper.length, "double") === null} onClick={() => {
                const latest = useWaveformHelper.getState()
                if (latest.fit !== "custom") return
                const next = nextWaveformLengthScale(latest.length, "double")
                if (next !== null) latest.configure({ length: next })
              }}>{t("Double")}</Button>
            </div>
          </Field>}
          <Field><FieldLabel>{t("Center MIDI key")}</FieldLabel><NumberField value={helper.anchorKey} min={0} max={127} step={1} aria-label={t("Waveform helper center key")} onValueChange={(anchorKey) => helper.configure({ anchorKey: Math.round(anchorKey) })} />
            <div className="flex gap-1">
              <Button type="button" variant="outline" size="sm" aria-label={t("Halve distance from C5")} disabled={nextWaveformCenterScale(helper.anchorKey, "half") === null} onClick={() => {
                const latest = useWaveformHelper.getState()
                const next = nextWaveformCenterScale(latest.anchorKey, "half")
                if (next !== null) latest.configure({ anchorKey: next })
              }}>{t("Half")}</Button>
              <Button type="button" variant="outline" size="sm" aria-label={t("Double distance from C5")} disabled={nextWaveformCenterScale(helper.anchorKey, "double") === null} onClick={() => {
                const latest = useWaveformHelper.getState()
                const next = nextWaveformCenterScale(latest.anchorKey, "double")
                if (next !== null) latest.configure({ anchorKey: next })
              }}>{t("Double")}</Button>
            </div>
          </Field>
          <Field><FieldLabel>{t("Height (rows)")}</FieldLabel><NumberField value={helper.height} min={2} max={128} step={1} aria-label={t("Waveform helper height")} onValueChange={(height) => helper.configure({ height: Math.round(height) })} />
            <div className="flex gap-1">
              <Button type="button" variant="outline" size="sm" disabled={nextWaveformHeightScale(helper.height, "half") === null} onClick={() => {
                const next = nextWaveformHeightScale(useWaveformHelper.getState().height, "half")
                if (next !== null) useWaveformHelper.getState().configure({ height: next })
              }}>{t("Half")}</Button>
              <Button type="button" variant="outline" size="sm" disabled={nextWaveformHeightScale(helper.height, "double") === null} onClick={() => {
                const next = nextWaveformHeightScale(useWaveformHelper.getState().height, "double")
                if (next !== null) useWaveformHelper.getState().configure({ height: next })
              }}>{t("Double")}</Button>
            </div>
          </Field>
          <Field><FieldLabel>{t("Opacity (%)")}</FieldLabel><NumberField value={helper.opacity * 100} min={1} max={60} step={1} aria-label={t("Waveform helper opacity")} onValueChange={(opacity) => helper.configure({ opacity: opacity / 100 })} />
            <div className="flex gap-1">
              <Button type="button" variant="outline" size="sm" aria-label={t("Halve waveform opacity")} disabled={nextWaveformOpacityScale(helper.opacity, "half") === null} onClick={() => {
                const next = nextWaveformOpacityScale(useWaveformHelper.getState().opacity, "half")
                if (next !== null) useWaveformHelper.getState().configure({ opacity: next })
              }}>{t("Half")}</Button>
              <Button type="button" variant="outline" size="sm" aria-label={t("Double waveform opacity")} disabled={nextWaveformOpacityScale(helper.opacity, "double") === null} onClick={() => {
                const next = nextWaveformOpacityScale(useWaveformHelper.getState().opacity, "double")
                if (next !== null) useWaveformHelper.getState().configure({ opacity: next })
              }}>{t("Double")}</Button>
            </div>
          </Field>
        </div>
        {asset && (!loaded || loaded.status === "loading") && <p role="status" className="text-xs text-muted-foreground">{t("Loading waveform…")}</p>}
        {loaded?.status === "error" && <p role="alert" className="text-xs text-destructive">{loaded.message}</p>}
        {helper.sample !== null && !asset && <p role="status" className="text-xs text-muted-foreground">{t("This reference audio is no longer in the project.")}</p>}
        {loaded?.status === "ready" && <p className="text-xs text-muted-foreground">{loaded.info.durationSecs.toFixed(2)}{" "}{t("seconds ·")}{" "}{loaded.info.sampleRate.toLocaleString()} Hz</p>}
        <Button type="button" size="sm" variant="ghost" onClick={() => helper.configure({ ...initial })}>{t("Clear reference")}</Button>
      </FieldGroup>
    </PopoverContent>
  </Popover>
}
