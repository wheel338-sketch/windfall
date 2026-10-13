import {
  useEffect,
  useId,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react"

import { noteName } from "@/components/audio"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { useProjectStore } from "@/lib/store/project"

import {
  applyRiff,
  auditionRiff,
  closeRiffGenerator,
  generateRiff,
  initialRiffSettings,
  riffRequestIsCurrent,
  stopRiffAudition,
  useRiffGenerator,
  type RiffRequest,
} from "../riff-generator"
import { ROOT_NAMES } from "../scales"
import {
  DEFAULT_RIFF_SETTINGS,
  RIFF_ARP_PATTERNS,
  RIFF_ARP_RATES,
  RIFF_BARS_PER_CHORD,
  RIFF_BASS,
  RIFF_CHORD_SIZES,
  RIFF_INVERSIONS,
  RIFF_INVERTS,
  RIFF_LENGTH_MODES,
  RIFF_PAN_MODES,
  RIFF_PLACEMENTS,
  RIFF_SCALES,
  RIFF_SPREADS,
  RIFF_STAGES,
  RiffError,
  type RiffSettings,
  type RiffStageId,
} from "./model"
import type { RiffResult } from "./pipeline"
import { RiffPreview } from "./preview"
import { MAX_RIFF_SEED } from "./rng"
import { CUSTOM_PRESET, degreeNumeral, RIFF_PRESETS } from "./theory"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const STAGE_HELP: Record<RiffStageId, string> = {
  get progression() { return t("Block chords from scale degrees. Every later stage works on these notes.") },
  get chords() { return t("Voices each chord: which note is lowest, how far the notes spread, and an optional bass note.") },
  get arpeggio() { return t("Plays each chord one note at a time. Switched off, chords stay as blocks.") },
  get mirror() { return t("Turns pitches upside down, or plays each chord backwards.") },
  get levels() { return t("Sets velocity, pan and per-note expression. Switched off, notes keep the draw velocity.") },
  get articulation() { return t("Changes how long notes are held.") },
  get groove() { return t("Moves note starts. Swing needs the arpeggio; no note leaves its chord.") },
  get fit() { return t("Keeps every note inside a key range and on the scale. MIDI 0–127 and the note limit always apply.") },
}

const GRID = "grid grid-cols-2 gap-x-4 gap-y-3 sm:grid-cols-3"

/** A whole number. The text is kept here so a half-typed value survives. */
function Num({
  label,
  value,
  set,
  min,
  max,
  disabled,
  help,
}: {
  label: string
  value: number
  set(value: number): void
  min: number
  max: number
  disabled: boolean
  help?: string
}) {
  const id = useId()
  const [text, setText] = useState(Number.isNaN(value) ? "" : String(value))
  const invalid = !Number.isInteger(value) || value < min || value > max
  return (
    <Field data-invalid={invalid} data-disabled={disabled}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        step={1}
        value={text}
        disabled={disabled}
        aria-invalid={invalid}
        onChange={(event) => {
          setText(event.target.value)
          set(event.target.value.trim() ? Number(event.target.value) : NaN)
        }}
      />
      {help && <FieldDescription>{help}</FieldDescription>}
    </Field>
  )
}

function Choices<T extends string>({
  label,
  value,
  set,
  items,
  disabled,
  help,
}: {
  label: string
  value: T
  set(value: T): void
  items: readonly { value: T; label: string; disabled?: boolean }[]
  disabled: boolean
  help?: string
}) {
  return (
    <Field data-disabled={disabled} className="col-span-full">
      <FieldLabel>{label}</FieldLabel>
      <ToggleGroup
        aria-label={label}
        value={[value]}
        variant="outline"
        size="sm"
        disabled={disabled}
        className="flex-wrap"
        onValueChange={(values) => {
          const next = items.find((item) => item.value === values[0])
          if (next) set(next.value)
        }}
      >
        {items.map((item) => (
          <ToggleGroupItem
            key={item.value}
            value={item.value}
            disabled={item.disabled}
          >
            {item.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      {help && <FieldDescription>{help}</FieldDescription>}
    </Field>
  )
}

function Dropdown<T extends string | number>({
  label,
  value,
  set,
  items,
  disabled,
}: {
  label: string
  value: T
  set(value: T): void
  items: readonly { value: T; label: string }[]
  disabled: boolean
}) {
  const id = useId()
  return (
    <Field data-disabled={disabled}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Select
        value={value}
        items={items}
        disabled={disabled}
        onValueChange={(next) => {
          if (next !== null) set(next)
        }}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
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
    </Field>
  )
}

function Flag({
  label,
  checked,
  set,
  disabled,
}: {
  label: string
  checked: boolean
  set(checked: boolean): void
  disabled: boolean
}) {
  const id = useId()
  return (
    <Field orientation="horizontal" data-disabled={disabled}>
      <Switch
        id={id}
        size="sm"
        checked={checked}
        disabled={disabled}
        onCheckedChange={(next) => set(next)}
      />
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
    </Field>
  )
}

function RiffForm({ request }: { request: RiffRequest }) {
  const id = useId()
  // `epoch` remounts the number fields when their values change from
  // outside them: a reset, or a new seed.
  const [form, setForm] = useState(() => ({
    settings: initialRiffSettings(request),
    epoch: 0,
  }))
  const [tab, setTab] = useState<RiffStageId>("progression")
  const [failure, setFailure] = useState<string | null>(null)
  const pending = useRiffGenerator((state) => state.pending)
  const auditioning = useRiffGenerator((state) => state.auditioning)
  useProjectStore((state) => state.revision)
  useSyncExternalStore(
    (listener) => request.session.editor.subscribe(listener),
    () => riffRequestIsCurrent(request)
  )
  const current = riffRequestIsCurrent(request)
  useEffect(() => stopRiffAudition, [])
  useEffect(() => {
    if (!current) stopRiffAudition()
  }, [current])

  const { settings, epoch } = form
  let result: RiffResult | null = null
  let error: { stage: RiffStageId | null; message: string } | null = null
  try {
    result = generateRiff(request, settings)
  } catch (cause) {
    error =
      cause instanceof RiffError
        ? { stage: cause.stage, message: cause.message }
        : { stage: null, message: t("Check the riff settings.") }
  }

  /** Any change stops the audition: it would no longer match the preview. */
  function change(next: (settings: RiffSettings) => RiffSettings, remount = false) {
    stopRiffAudition()
    setFailure(null)
    setForm((previous) => ({
      settings: next(previous.settings),
      epoch: previous.epoch + (remount ? 1 : 0),
    }))
  }
  function stage<Id extends RiffStageId>(
    stageId: Id,
    patch: Partial<RiffSettings[Id]>,
    remount = false
  ) {
    change(
      (previous) => ({
        ...previous,
        [stageId]: { ...previous[stageId], ...patch },
      }),
      remount
    )
  }

  const { progression, chords, arpeggio, mirror, levels, articulation, groove, fit } =
    settings
  const errorLabel = RIFF_STAGES.find((item) => item.id === error?.stage)?.label
  const keys = result ? result.notes.map((note) => note.key) : []
  const presets = [
    ...RIFF_PRESETS.map((preset) => ({
      value: preset.id as string,
      label: `${preset.name} · ${preset.degrees
        .map((degree) =>
          degreeNumeral({ degree: degree - 1 }, progression.scale)
        )
        .join("–")}`,
    })),
    { value: CUSTOM_PRESET, label: t("Custom degrees…") },
  ]

  function panel(stageId: RiffStageId, fields: (off: boolean) => ReactNode) {
    const info = RIFF_STAGES.find((item) => item.id === stageId)!
    const on = stageId === "progression" || settings[stageId].enabled
    const count = result?.counts[stageId] ?? null
    return (
      <TabsContent value={stageId} className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {stageId !== "progression" ? (
            <Flag
              label={message("{infoLabel} stage", { infoLabel: info.label })}
              checked={on}
              set={(enabled) =>
                change((previous) => ({
                  ...previous,
                  [stageId]: { ...previous[stageId], enabled },
                }))
              }
              disabled={pending}
            />
          ) : (
            <span className="font-medium">{t("Always runs")}</span>
          )}
          <span className="text-muted-foreground">
            {!on
              ? t("Skipped")
              : count === null
                ? ""
                : message("{count} {count2} out", { count: plainNumber(count), count2: count === 1 ? t("note") : t("notes") })}
          </span>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="ml-auto"
            disabled={pending}
            onClick={() =>
              change(
                (previous) => ({
                  ...previous,
                  [stageId]: DEFAULT_RIFF_SETTINGS[stageId],
                }),
                true
              )
            }
          >
            {t("Reset stage")}
          </Button>
        </div>
        <p className="text-muted-foreground">{STAGE_HELP[stageId]}</p>
        <div className={GRID}>{fields(pending || !on)}</div>
      </TabsContent>
    )
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) closeRiffGenerator()
      }}
    >
      <DialogContent
        showCloseButton={!pending}
        className="max-h-[92dvh] overflow-y-auto sm:max-w-3xl"
      >
        <form
          className="flex flex-col gap-4"
          onSubmit={async (event) => {
            event.preventDefault()
            if (error || !current || pending) return
            setFailure(null)
            try {
              if (!(await applyRiff(request, settings)))
                setFailure(
                  "The riff could not be added. Close and reopen the generator to try again."
                )
            } catch (cause) {
              setFailure(
                cause instanceof Error
                  ? cause.message
                  : "The riff could not be added."
              )
            }
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("Generate riff")}</DialogTitle>
            <DialogDescription>
              {t("Build a phrase in eight stages. Each stage reshapes the notes of the one before it. Nothing is written until you apply.")}
            </DialogDescription>
          </DialogHeader>

          <div className="flex flex-col gap-1.5">
            <RiffPreview result={result} />
            {result && (
              <>
                <p role="status">
                  {result.notes.length}{" "}
                  {result.notes.length === 1 ? t("note") : t("notes")} ·{" "}
                  {progression.bars} {progression.bars === 1 ? t("bar") : t("bars")}{" "}
                  {t("of")}{" "}{result.context.barTicks}{" "}{t("ticks from tick")}{" "}{result.start} ·{" "}
                  {noteName(Math.min(...keys))}{" "}{t("to")}{" "}
                  {noteName(Math.max(...keys))}{" "}{t("· one undo step")}
                </p>
                <p className="truncate text-muted-foreground">
                  {t("Chords:")}{" "}{result.chords.slice(0, 16).join(" – ")}
                  {result.chords.length > 16 ? " …" : ""}
                </p>
              </>
            )}
          </div>

          <Tabs
            key={epoch}
            value={tab}
            onValueChange={(value) => setTab(value as RiffStageId)}
          >
            <TabsList className="w-full" aria-label={t("Stages")}>
              {RIFF_STAGES.map((item, index) => {
                const on =
                  item.id === "progression" || settings[item.id].enabled
                return (
                  <TabsTrigger key={item.id} value={item.id}>
                    <span
                      aria-hidden
                      className={
                        on ? "text-primary" : "text-muted-foreground/60"
                      }
                    >
                      {index + 1}
                    </span>
                    {item.label}
                    {!on && <span className="sr-only">{" "}{t("(off)")}</span>}
                  </TabsTrigger>
                )
              })}
            </TabsList>

            {panel("progression", (off) => (
              <>
                <Dropdown
                  label={t("Key")}
                  value={progression.root}
                  set={(root) => stage("progression", { root })}
                  items={ROOT_NAMES.map((label, value) => ({ label, value }))}
                  disabled={off}
                />
                <Dropdown
                  label={t("Scale")}
                  value={progression.scale}
                  set={(scale) => stage("progression", { scale })}
                  items={RIFF_SCALES}
                  disabled={off}
                />
                <Dropdown
                  label={t("Chord progression")}
                  value={progression.preset}
                  set={(preset) => stage("progression", { preset })}
                  items={presets}
                  disabled={off}
                />
                {progression.preset === CUSTOM_PRESET && (
                  <Field
                    className="col-span-full"
                    data-invalid={error?.stage === "progression"}
                    data-disabled={off}
                  >
                    <FieldLabel htmlFor={`${id}-custom`}>
                      {t("Custom degrees")}
                    </FieldLabel>
                    <Input
                      id={`${id}-custom`}
                      value={progression.custom}
                      disabled={off}
                      placeholder="I V vi IV"
                      aria-invalid={error?.stage === "progression"}
                      onChange={(event) =>
                        stage("progression", { custom: event.target.value })
                      }
                    />
                    <FieldDescription>
                      {t("Roman numerals or 1–7, separated by spaces, commas or dashes. Each chord's quality comes from the scale; add 7 or 9 to a numeral for a larger chord.")}
                    </FieldDescription>
                  </Field>
                )}
                <Choices
                  label={t("Chord size")}
                  value={progression.chordSize}
                  set={(chordSize) => stage("progression", { chordSize })}
                  items={RIFF_CHORD_SIZES}
                  disabled={off}
                />
                <Choices
                  label={t("Each chord lasts")}
                  value={progression.barsPerChord}
                  set={(barsPerChord) => stage("progression", { barsPerChord })}
                  items={RIFF_BARS_PER_CHORD}
                  disabled={off}
                />
                <Num
                  label={t("Length (bars)")}
                  value={progression.bars}
                  set={(bars) => stage("progression", { bars })}
                  min={1}
                  max={16}
                  disabled={off}
                />
                <Num
                  label={t("Octave")}
                  value={progression.octave}
                  set={(octave) => stage("progression", { octave })}
                  min={1}
                  max={8}
                  disabled={off}
                />
                <Num
                  label={t("Variation (%)")}
                  value={progression.variation}
                  set={(variation) => stage("progression", { variation })}
                  min={0}
                  max={100}
                  disabled={off}
                  help={t("Chance that a chord after the first is swapped for one a third away.")}
                />
              </>
            ))}

            {panel("chords", (off) => (
              <>
                <Choices
                  label={t("Inversion")}
                  value={chords.inversion}
                  set={(inversion) => stage("chords", { inversion })}
                  items={RIFF_INVERSIONS}
                  disabled={off}
                  help={t("Smooth picks, for each chord, the inversion nearest the chord before it.")}
                />
                <Choices
                  label={t("Spread")}
                  value={chords.spread}
                  set={(spread) => stage("chords", { spread })}
                  items={RIFF_SPREADS}
                  disabled={off}
                />
                <Choices
                  label={t("Bass note")}
                  value={chords.bass}
                  set={(bass) => stage("chords", { bass })}
                  items={RIFF_BASS}
                  disabled={off}
                />
                <Num
                  label={t("Strum (ticks)")}
                  value={chords.strum}
                  set={(strum) => stage("chords", { strum })}
                  min={-480}
                  max={480}
                  disabled={off}
                  help={t("Delay between chord notes. Negative strums downward.")}
                />
              </>
            ))}

            {panel("arpeggio", (off) => (
              <>
                <Choices
                  label={t("Pattern")}
                  value={arpeggio.pattern}
                  set={(pattern) => stage("arpeggio", { pattern })}
                  items={RIFF_ARP_PATTERNS}
                  disabled={off}
                />
                <Choices
                  label={t("Rate")}
                  value={arpeggio.rate}
                  set={(rate) => stage("arpeggio", { rate })}
                  items={RIFF_ARP_RATES}
                  disabled={off}
                />
                <Num
                  label={t("Octave range")}
                  value={arpeggio.octaves}
                  set={(octaves) => stage("arpeggio", { octaves })}
                  min={1}
                  max={3}
                  disabled={off}
                />
                <Num
                  label={t("Gate (%)")}
                  value={arpeggio.gate}
                  set={(gate) => stage("arpeggio", { gate })}
                  min={1}
                  max={100}
                  disabled={off}
                />
                <Flag
                  label={t("Restart at each chord")}
                  checked={arpeggio.restart}
                  set={(restart) => stage("arpeggio", { restart })}
                  disabled={off}
                />
              </>
            ))}

            {panel("mirror", (off) => (
              <>
                <Choices
                  label={t("Flip pitch")}
                  value={mirror.invert}
                  set={(invert) => stage("mirror", { invert })}
                  items={RIFF_INVERTS}
                  disabled={off}
                />
                <Num
                  label={t("Mirror key (MIDI)")}
                  value={mirror.axis}
                  set={(axis) => stage("mirror", { axis })}
                  min={0}
                  max={127}
                  disabled={off || mirror.invert !== "key"}
                />
                <Flag
                  label={t("Keep flipped notes in the scale")}
                  checked={mirror.keepInScale}
                  set={(keepInScale) => stage("mirror", { keepInScale })}
                  disabled={off || mirror.invert === "off"}
                />
                <Flag
                  label={t("Reverse each chord in time")}
                  checked={mirror.reverse}
                  set={(reverse) => stage("mirror", { reverse })}
                  disabled={off}
                />
              </>
            ))}

            {panel("levels", (off) => (
              <>
                <Num
                  label={t("Velocity (%)")}
                  value={levels.base}
                  set={(base) => stage("levels", { base })}
                  min={0}
                  max={100}
                  disabled={off}
                />
                <Num
                  label={t("Accent every (steps)")}
                  value={levels.accentEvery}
                  set={(accentEvery) => stage("levels", { accentEvery })}
                  min={0}
                  max={16}
                  disabled={off}
                  help={t("0 turns the accent off.")}
                />
                <Num
                  label={t("Accent amount (± points)")}
                  value={levels.accentAmount}
                  set={(accentAmount) => stage("levels", { accentAmount })}
                  min={-100}
                  max={100}
                  disabled={off}
                />
                <Num
                  label={t("Random velocity (± points)")}
                  value={levels.random}
                  set={(random) => stage("levels", { random })}
                  min={0}
                  max={100}
                  disabled={off}
                />
                <Num
                  label={t("Ramp (points)")}
                  value={levels.ramp}
                  set={(ramp) => stage("levels", { ramp })}
                  min={-100}
                  max={100}
                  disabled={off}
                  help={t("Positive grows louder across the phrase, negative quieter.")}
                />
                <Choices
                  label={t("Pan")}
                  value={levels.panMode}
                  set={(panMode) => stage("levels", { panMode })}
                  items={RIFF_PAN_MODES}
                  disabled={off}
                />
                <Num
                  label={t("Pan amount (%)")}
                  value={levels.panAmount}
                  set={(panAmount) => stage("levels", { panAmount })}
                  min={0}
                  max={100}
                  disabled={off || levels.panMode === "centre"}
                />
                <Num
                  label={t("Release (%)")}
                  value={levels.release}
                  set={(release) => stage("levels", { release })}
                  min={0}
                  max={100}
                  disabled={off}
                  help={t("50 is neutral.")}
                />
                <Num
                  label={t("Modulation X (%)")}
                  value={levels.modX}
                  set={(modX) => stage("levels", { modX })}
                  min={0}
                  max={100}
                  disabled={off}
                />
                <Num
                  label={t("Modulation Y (%)")}
                  value={levels.modY}
                  set={(modY) => stage("levels", { modY })}
                  min={0}
                  max={100}
                  disabled={off}
                />
              </>
            ))}

            {panel("articulation", (off) => (
              <>
                <Choices
                  label={t("Length")}
                  value={articulation.mode}
                  set={(mode) => stage("articulation", { mode })}
                  items={RIFF_LENGTH_MODES}
                  disabled={off}
                  help={t("Legato holds each note until the next one starts. Staccato halves every note.")}
                />
                <Num
                  label={t("Multiplier (%)")}
                  value={articulation.multiplier}
                  set={(multiplier) => stage("articulation", { multiplier })}
                  min={10}
                  max={400}
                  disabled={off || articulation.mode !== "scale"}
                />
                <Num
                  label={t("Minimum length (ticks)")}
                  value={articulation.minLength}
                  set={(minLength) => stage("articulation", { minLength })}
                  min={1}
                  max={960}
                  disabled={off}
                />
                <Flag
                  label={t("Tie repeated pitches")}
                  checked={articulation.tie}
                  set={(tie) => stage("articulation", { tie })}
                  disabled={off}
                />
              </>
            ))}

            {panel("groove", (off) => (
              <>
                <Num
                  label={t("Swing (%)")}
                  value={groove.swing}
                  set={(swing) => stage("groove", { swing })}
                  min={0}
                  max={100}
                  disabled={off}
                  help={t("Delays every second arpeggio step. 100 is a triplet feel.")}
                />
                <Num
                  label={t("Timing variation (± ticks)")}
                  value={groove.humanize}
                  set={(humanize) => stage("groove", { humanize })}
                  min={0}
                  max={120}
                  disabled={off}
                />
              </>
            ))}

            {panel("fit", (off) => (
              <>
                <Num
                  label={t("Lowest key (MIDI)")}
                  value={fit.low}
                  set={(low) => stage("fit", { low })}
                  min={0}
                  max={127}
                  disabled={off}
                />
                <Num
                  label={t("Highest key (MIDI)")}
                  value={fit.high}
                  set={(high) => stage("fit", { high })}
                  min={0}
                  max={127}
                  disabled={off}
                />
                <Flag
                  label={t("Keep notes in the scale")}
                  checked={fit.keepInScale}
                  set={(keepInScale) => stage("fit", { keepInScale })}
                  disabled={off}
                />
                <Flag
                  label={t("Drop duplicate notes")}
                  checked={fit.dropDuplicates}
                  set={(dropDuplicates) => stage("fit", { dropDuplicates })}
                  disabled={off}
                />
              </>
            ))}
          </Tabs>

          <div key={`shared-${epoch}`} className={GRID}>
            <Num
              label={t("Seed")}
              value={settings.seed}
              set={(seed) => change((previous) => ({ ...previous, seed }))}
              min={0}
              max={MAX_RIFF_SEED}
              disabled={pending}
              help={t("The same settings and seed always give the same riff.")}
            />
            <div className="flex items-start pt-5">
              <Button
                type="button"
                variant="outline"
                disabled={pending}
                onClick={() =>
                  change(
                    (previous) => ({
                      ...previous,
                      seed: Math.floor(Math.random() * (MAX_RIFF_SEED + 1)),
                    }),
                    true
                  )
                }
              >
                {t("Randomise seed")}
              </Button>
            </div>
            <Flag
              label={t("Extend the pattern to fit")}
              checked={settings.extendPattern}
              set={(extendPattern) =>
                change((previous) => ({ ...previous, extendPattern }))
              }
              disabled={pending}
            />
            <Choices
              label={t("Place the riff")}
              value={settings.placement}
              set={(placement) =>
                change((previous) => ({ ...previous, placement }))
              }
              items={RIFF_PLACEMENTS.map((item) => ({
                ...item,
                disabled:
                  item.value === "selection" && request.selection.length === 0,
              }))}
              disabled={pending}
              help={
                request.selection.length === 0
                  ? t("Select notes before opening the generator to replace them.")
                  : message("Replacing removes the {count} selected {count2} and starts at the first of them.", { count: plainNumber(request.selection.length), count2: request.selection.length === 1 ? t("note") : t("notes") })
              }
            />
          </div>

          {(error || failure || !current) && (
            <Alert variant="destructive">
              <AlertDescription>
                {!current
                  ? t("The project or piano-roll channel changed. Close and reopen the generator.")
                  : error
                    ? errorLabel ? message("{name}: {value}", { name: errorLabel, value: error.message }) : error.message
                    : failure}
              </AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              className="sm:mr-auto"
              disabled={!!error || !current || pending}
              aria-pressed={auditioning}
              onClick={() => {
                if (auditioning) return stopRiffAudition()
                try {
                  auditionRiff(request, settings)
                } catch (cause) {
                  setFailure(
                    cause instanceof Error
                      ? cause.message
                      : "The riff could not be played."
                  )
                }
              }}
            >
              {auditioning ? t("Stop") : t("Audition")}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={closeRiffGenerator}
            >
              {t("Cancel")}
            </Button>
            <Button type="submit" disabled={!!error || !current || pending}>
              {pending ? t("Applying…") : t("Apply")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function RiffGeneratorDialog() {
  const request = useRiffGenerator((state) => state.request)
  return request ? (
    <RiffForm
      key={`${request.generation}:${request.revision}:${request.context.pattern.id}:${request.context.channel}`}
      request={request}
    />
  ) : null
}
