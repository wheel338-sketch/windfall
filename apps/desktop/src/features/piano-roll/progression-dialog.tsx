import { useId, useState, useSyncExternalStore } from "react"

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
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { useProjectStore } from "@/lib/store/project"

import { ROOT_NAMES } from "./scales"
import {
  applyProgression,
  closeProgressionGenerator,
  PROGRESSION_MOODS,
  PROGRESSION_MODES,
  progressionCommand,
  progressionRequestIsCurrent,
  useProgressionGenerator,
  type ProgressionRequest,
  type ProgressionSettings,
} from "./progression"
import { translatedText as t } from "@/lib/i18n"

function Choices<T extends string>({
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
  return (
    <Field data-disabled={disabled}>
      <FieldLabel>{label}</FieldLabel>
      <ToggleGroup
        aria-label={label}
        value={[value]}
        variant="outline"
        size="sm"
        disabled={disabled}
        onValueChange={(values) => {
          const next = items.find((item) => item.value === values[0])
          if (next) set(next.value)
        }}
      >
        {items.map((item) => (
          <ToggleGroupItem key={item.value} value={item.value}>
            {item.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
    </Field>
  )
}

function ProgressionForm({ request }: { request: ProgressionRequest }) {
  const id = useId()
  const [root, setRoot] = useState(0)
  const [mode, setMode] = useState<ProgressionSettings["mode"]>("major")
  const [bars, setBars] = useState("4")
  const [mood, setMood] = useState<ProgressionSettings["mood"]>("bright")
  const [seed, setSeed] = useState("0")
  const [failure, setFailure] = useState<string | null>(null)
  const pending = useProgressionGenerator((state) => state.pending)
  useProjectStore((state) => state.revision)
  useSyncExternalStore(
    (listener) => request.session.editor.subscribe(listener),
    () => progressionRequestIsCurrent(request)
  )
  const current = progressionRequestIsCurrent(request)
  const settings: ProgressionSettings = {
    root,
    mode,
    bars: bars.trim() ? Number(bars) : NaN,
    mood,
    seed: seed.trim() ? Number(seed) : NaN,
  }
  let error: string | null = null
  let count = 0
  try {
    const command = progressionCommand(request, settings)
    if (command.type === "addNotes") count = command.notes.length
  } catch (cause) {
    error =
      cause instanceof Error ? cause.message : "Check the progression settings."
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) closeProgressionGenerator()
      }}
    >
      <DialogContent
        showCloseButton={!pending}
        className="max-h-[85dvh] overflow-y-auto sm:max-w-lg"
      >
        <form
          className="flex flex-col gap-4"
          onSubmit={async (event) => {
            event.preventDefault()
            if (error || !current || pending) return
            setFailure(null)
            try {
              if (!(await applyProgression(request, settings)))
                setFailure(
                  "The progression could not be added. Close and reopen the generator to try again."
                )
            } catch (cause) {
              setFailure(
                cause instanceof Error
                  ? cause.message
                  : "The progression could not be added."
              )
            }
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("Generate progression")}</DialogTitle>
            <DialogDescription>
              {t("Append diatonic triads from a fixed mood cycle after this channel's notes.")}
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field data-disabled={pending}>
              <FieldLabel htmlFor={`${id}-root`}>{t("Root")}</FieldLabel>
              <Select
                value={root}
                items={ROOT_NAMES.map((label, value) => ({ label, value }))}
                disabled={pending}
                onValueChange={(value) => {
                  if (value !== null) setRoot(value)
                }}
              >
                <SelectTrigger id={`${id}-root`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {ROOT_NAMES.map((label, value) => (
                      <SelectItem key={value} value={value}>
                        {label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            <Choices<ProgressionSettings["mode"]>
              label={t("Mode")}
              value={mode}
              set={setMode}
              items={PROGRESSION_MODES}
              disabled={pending}
            />
            <Field
              data-invalid={
                !Number.isInteger(settings.bars) ||
                settings.bars < 2 ||
                settings.bars > 8
              }
              data-disabled={pending}
            >
              <FieldLabel htmlFor={`${id}-bars`}>{t("Length (bars)")}</FieldLabel>
              <Input
                id={`${id}-bars`}
                type="number"
                min={2}
                max={8}
                step={1}
                value={bars}
                disabled={pending}
                aria-invalid={
                  !Number.isInteger(settings.bars) ||
                  settings.bars < 2 ||
                  settings.bars > 8
                }
                onChange={(event) => setBars(event.target.value)}
              />
            </Field>
            <Choices<ProgressionSettings["mood"]>
              label={t("Mood")}
              value={mood}
              set={setMood}
              items={PROGRESSION_MOODS}
              disabled={pending}
            />
            <Field
              data-invalid={
                !Number.isSafeInteger(settings.seed) ||
                settings.seed < 0 ||
                settings.seed > Number.MAX_SAFE_INTEGER
              }
              data-disabled={pending}
            >
              <FieldLabel htmlFor={`${id}-seed`}>{t("Seed")}</FieldLabel>
              <Input
                id={`${id}-seed`}
                type="number"
                min={0}
                max={Number.MAX_SAFE_INTEGER}
                step={1}
                value={seed}
                disabled={pending}
                aria-invalid={
                  !Number.isSafeInteger(settings.seed) ||
                  settings.seed < 0 ||
                  settings.seed > Number.MAX_SAFE_INTEGER
                }
                onChange={(event) => setSeed(event.target.value)}
              />
              <FieldDescription>
                {t("Use the same settings and seed to repeat the chords. Meter and draw velocity are captured when this dialog opens.")}
              </FieldDescription>
            </Field>
          </FieldGroup>
          {!error && (
            <p role="status">
              {count}{" "}{t("notes ·")}{" "}{request.barTicks}{" "}{t("ticks per chord · one undo step")}
            </p>
          )}
          {(error || failure || !current) && (
            <Alert variant="destructive">
              <AlertDescription>
                {!current
                  ? t("The project or piano-roll channel changed. Close and reopen the generator.")
                  : (error ?? failure)}
              </AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={closeProgressionGenerator}
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

export function ProgressionGeneratorDialog() {
  const request = useProgressionGenerator((state) => state.request)
  return request ? (
    <ProgressionForm
      key={`${request.generation}:${request.revision}:${request.context.pattern.id}:${request.context.channel}`}
      request={request}
    />
  ) : null
}
