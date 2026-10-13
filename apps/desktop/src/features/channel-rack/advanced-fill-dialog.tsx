import { useId, useRef, useState } from "react"

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
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { TICKS_PER_STEP } from "@/lib/units"

import {
  applyAdvancedFill,
  closeAdvancedFill,
  fillPreview,
  fillRequestIsCurrent,
  useAdvancedFill,
  type FillOptions,
  type FillRequest,
  type FillRule,
} from "./advanced-fill"
import { nextFillRule } from "./fill-rule-step"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const number = (value: string) => (value.trim() === "" ? NaN : Number(value))

function NumericField({
  label,
  value,
  set,
  min,
  max,
  step = 1,
}: {
  label: string
  value: string
  set(value: string): void
  min: number
  max: number
  step?: number
}) {
  const id = useId()
  const n = number(value)
  const valid =
    Number.isFinite(n) &&
    n >= min &&
    n <= max &&
    (step !== 1 || Number.isInteger(n))
  return (
    <Field data-invalid={!valid}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type="number"
        value={value}
        min={min}
        max={max}
        step={step}
        aria-invalid={!valid}
        onChange={(event) => set(event.target.value)}
      />
    </Field>
  )
}

function FillForm({ request }: { request: FillRequest }) {
  const [rule, setRule] = useState<FillRule>("euclidean")
  const [start, setStart] = useState("1")
  const [end, setEnd] = useState(String(request.lengthSteps))
  const [cycle, setCycle] = useState(String(Math.min(16, request.lengthSteps)))
  const [hits, setHits] = useState(String(Math.min(5, request.lengthSteps)))
  const [every, setEvery] = useState(String(Math.min(4, request.lengthSteps)))
  const [rotation, setRotation] = useState("0")
  const [seed, setSeed] = useState("1")
  const [key, setKey] = useState("60")
  const [velocity, setVelocity] = useState("80")
  const [gate, setGate] = useState("100")
  const [replace, setReplace] = useState(true)
  const [pending, setPending] = useState(false)
  const [failure, setFailure] = useState(false)
  const submitting = useRef(false)
  // Render the refusal as soon as any relevant document/selection changes.
  useProjectStore((state) => state.revision)
  useUiStore((state) => state.selectedChannel)
  useTransportStore((state) => state.pattern)
  const current = fillRequestIsCurrent(request)
  const options: FillOptions = {
    rule,
    startStep: number(start) - 1,
    endStep: number(end),
    cycle: number(cycle),
    hits: number(hits),
    every: number(every),
    rotation: number(rotation),
    seed: number(seed),
    key: number(key),
    velocity: number(velocity) / 100,
    gate: number(gate) / 100,
    replace,
  }
  const preview = fillPreview(options, request.lengthSteps)
  const occupied = new Set(request.notes.map((note) => note.start))
  const additions =
    preview?.filter((note) => replace || !occupied.has(note.start)) ?? []
  const existing = request.notes.filter(
    (note) =>
      note.start >= options.startStep * TICKS_PER_STEP &&
      note.start < options.endStep * TICKS_PER_STEP
  )
  const visible = new Set(additions.map((note) => note.start / TICKS_PER_STEP))
  if (!replace)
    for (const note of existing)
      visible.add(Math.floor(note.start / TICKS_PER_STEP))
  const count = options.endStep - options.startStep
  const columns = Math.min(32, count)

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !submitting.current) closeAdvancedFill()
      }}
    >
      <DialogContent
        className="max-h-[85dvh] overflow-y-auto sm:max-w-lg"
        showCloseButton={!pending}
      >
        <DialogHeader>
          <DialogTitle>{t("Advanced step fill")}</DialogTitle>
          <DialogDescription>
            {request.channelName} · {request.patternName}{t(". Review the rhythm before applying it as one undo step.")}
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={async (event) => {
            event.preventDefault()
            if (!preview || !current || submitting.current) return
            submitting.current = true
            setPending(true)
            setFailure(false)
            try {
              if (!(await applyAdvancedFill(request, options))) setFailure(true)
            } finally {
              submitting.current = false
              setPending(false)
            }
          }}
        >
          <FieldGroup>
            <Field>
              <FieldLabel>{t("Rhythm rule")}</FieldLabel>
              <ToggleGroup
                aria-label={t("Rhythm rule")}
                value={[rule]}
                variant="outline"
                size="sm"
                disabled={pending}
                onValueChange={(values) => {
                  const value = values[0]
                  if (
                    value === "regular" ||
                    value === "euclidean" ||
                    value === "random"
                  )
                    setRule(value)
                }}
              >
                <ToggleGroupItem value="regular">{t("Regular")}</ToggleGroupItem>
                <ToggleGroupItem value="euclidean">{t("Euclidean")}</ToggleGroupItem>
                <ToggleGroupItem value="random">{t("Seeded random")}</ToggleGroupItem>
              </ToggleGroup>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={t("Choose the previous rhythm rule")}
                  disabled={pending || nextFillRule(rule, "previous") === null}
                  onClick={() => {
                    const next = nextFillRule(rule, "previous")
                    if (next !== null) setRule(next)
                  }}
                >
                  {t("Previous")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={t("Choose the next rhythm rule")}
                  disabled={pending || nextFillRule(rule, "next") === null}
                  onClick={() => {
                    const next = nextFillRule(rule, "next")
                    if (next !== null) setRule(next)
                  }}
                >
                  {t("Next")}
                </Button>
              </div>
              <FieldDescription>
                {rule === "regular"
                  ? t("Start each cycle with a hit, then repeat at the chosen spacing.")
                  : rule === "euclidean"
                    ? t("Distribute the chosen number of hits as evenly as possible around each cycle.")
                    : t("Choose an exact number of distinct hits per cycle. The same seed gives the same rhythm.")}{" "}
                {t("The cycle repeats from the first step and stops at the last step.")}
              </FieldDescription>
            </Field>
            <FieldGroup className="grid grid-cols-2 gap-3">
              <NumericField
                label={t("First step")}
                value={start}
                set={setStart}
                min={1}
                max={request.lengthSteps}
              />
              <NumericField
                label={t("Last step")}
                value={end}
                set={setEnd}
                min={number(start)}
                max={request.lengthSteps}
              />
              <NumericField
                label={t("Cycle steps")}
                value={cycle}
                set={setCycle}
                min={1}
                max={request.lengthSteps}
              />
              {rule === "regular" ? (
                <NumericField
                  label={t("Hit every (steps)")}
                  value={every}
                  set={setEvery}
                  min={1}
                  max={number(cycle)}
                />
              ) : (
                <NumericField
                  label={t("Hits per cycle")}
                  value={hits}
                  set={setHits}
                  min={0}
                  max={number(cycle)}
                />
              )}
              <NumericField
                label={t("Rotate right (steps)")}
                value={rotation}
                set={setRotation}
                min={0}
                max={number(cycle) - 1}
              />
              {rule === "random" && (
                <NumericField
                  label={t("Random seed")}
                  value={seed}
                  set={setSeed}
                  min={0}
                  max={0xffffffff}
                />
              )}
              <NumericField
                label={t("MIDI key")}
                value={key}
                set={setKey}
                min={0}
                max={127}
              />
              <NumericField
                label={t("Velocity (%)")}
                value={velocity}
                set={setVelocity}
                min={0}
                max={100}
                step={0.1}
              />
              <NumericField
                label={t("Note length (%)")}
                value={gate}
                set={setGate}
                min={0.1}
                max={100}
                step={0.1}
              />
            </FieldGroup>
            <Field>
              <FieldLabel>{t("Existing notes")}</FieldLabel>
              <ToggleGroup
                aria-label={t("Existing notes")}
                value={[replace ? "replace" : "overlay"]}
                variant="outline"
                size="sm"
                disabled={pending}
                onValueChange={(values) => {
                  if (values[0] === "replace" || values[0] === "overlay")
                    setReplace(values[0] === "replace")
                }}
              >
                <ToggleGroupItem value="replace">{t("Replace range")}</ToggleGroupItem>
                <ToggleGroupItem value="overlay">{t("Add to range")}</ToggleGroupItem>
              </ToggleGroup>
              <FieldDescription>
                {replace
                  ? t("Replace notes starting in this range, including piano roll notes and chords.")
                  : t("Keep all notes. Generated hits at an occupied onset are skipped.")}{" "}
                {t("Notes starting outside the range stay.")}
              </FieldDescription>
            </Field>
          </FieldGroup>
          {preview && (
            <figure className="flex flex-col gap-2 rounded-md border p-3">
              <figcaption
                className="text-xs text-muted-foreground"
                aria-live="polite"
              >
                {replace
                  ? message("{count} hits in steps {start}–{end}; {count2} existing notes reviewed for replacement.", { count: plainNumber(preview.length), start, end, count2: plainNumber(existing.length) })
                  : message("{count} new hits; {count2} occupied onsets skipped.", { count: plainNumber(additions.length), count2: plainNumber(preview.length - additions.length) })}
              </figcaption>
              <div className="max-h-28 overflow-y-auto">
                <svg
                  role="img"
                  aria-label={message("Step fill preview: {count} occupied steps", { count: plainNumber(visible.size) })}
                  viewBox={`0 0 ${columns * 12} ${Math.ceil(count / columns) * 12}`}
                  className="block w-full"
                >
                  {Array.from({ length: count }, (_, i) => {
                    const step = options.startStep + i
                    return (
                      <rect
                        key={step}
                        data-fill-step={step + 1}
                        data-lit={visible.has(step)}
                        x={(i % columns) * 12}
                        y={Math.floor(i / columns) * 12}
                        width={10}
                        height={10}
                        rx={2}
                        className={
                          visible.has(step) ? "fill-primary" : "fill-muted"
                        }
                      />
                    )
                  })}
                </svg>
              </div>
            </figure>
          )}
          {!current && (
            <Alert variant="destructive">
              <AlertDescription>
                {t("The document or selected lane changed. Cancel and reopen Advanced step fill to review it again.")}
              </AlertDescription>
            </Alert>
          )}
          {failure && (
            <Alert variant="destructive">
              <AlertDescription>
                {t("The fill could not be applied. Your notes were kept. Cancel and reopen the tool to review the current lane.")}
              </AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={closeAdvancedFill}
            >
              {t("Cancel")}
            </Button>
            <Button type="submit" disabled={!preview || !current || pending}>
              {pending ? t("Applying…") : t("Apply")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function AdvancedFillDialog() {
  const request = useAdvancedFill((state) => state.request)
  return request ? <FillForm key={request.id} request={request} /> : null
}
