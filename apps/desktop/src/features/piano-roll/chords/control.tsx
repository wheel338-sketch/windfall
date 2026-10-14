import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react"

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
import {
  dispatch,
  onHistoryNavigation,
  useProjectStore,
} from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { DEFAULT_KEY, MAX_PATTERN_TICKS } from "@/lib/units"

import { useSession } from "../context"
import type { EditorContext } from "../editor"
import { usePianoRollStore } from "../store"
import { chordInsertCommand } from "./command"
import {
  detectChord,
  pitchClass,
  PITCH_NAMES,
  TRIADS,
  type TriadQuality,
} from "./detection"
import {
  advanceSeed,
  MAX_SEED,
  MIN_SEED,
  seededTriad,
  triadKeys,
} from "./generator"
import { message, translatedText as t } from "@/lib/i18n"

type Request = {
  context: EditorContext
  generation: number
  revision: number
  root: number
  start: number
  length: number
}

function whole(value: string): number {
  return value.trim() ? Number(value) : NaN
}

function ChordForm({ request, close }: { request: Request; close(): void }) {
  const session = useSession()
  const { editor } = session
  // A string snapshot stays stable when the detected pitch-class set is unchanged.
  useSyncExternalStore(
    (listener) => editor.subscribe(listener),
    () =>
      `${editor.context?.pattern.id}:${editor.context?.channel}:${editor.busy}:${detectChord(editor.selectedNotes().map((note) => note.key)).label}`
  )
  const detection = detectChord(editor.selectedNotes().map((note) => note.key))
  const revision = useProjectStore((state) => state.revision)
  const [root, setRoot] = useState(String(request.root))
  const [start, setStart] = useState(String(request.start))
  const [length, setLength] = useState(String(request.length))
  const [seed, setSeed] = useState("0")
  const [quality, setQuality] = useState<TriadQuality>("major")
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const submitting = useRef(false)
  const id = useId()
  const current =
    revision === request.revision &&
    getProjectGeneration() === request.generation &&
    editor.context?.pattern.id === request.context.pattern.id &&
    editor.context?.channel === request.context.channel &&
    !editor.busy

  function preview(generated: boolean) {
    try {
      const choice = generated ? seededTriad(whole(seed), whole(root)) : null
      const keys = choice?.keys ?? triadKeys(whole(root), quality)
      const command = chordInsertCommand(
        request.context,
        keys,
        whole(start),
        whole(length)
      )
      const chordRoot = choice?.root ?? whole(root)
      const chordQuality = TRIADS.find(
        (triad) => triad.quality === (choice?.quality ?? quality)
      )!
      const chord = message("{root} {quality}", { root: PITCH_NAMES[pitchClass(chordRoot)], quality: chordQuality.label.toLowerCase() })
      const label = [...(choice ? [choice.degree] : []), chord, `MIDI ${keys.join(", ")}`].join(" · ")
      return { command, label, error: null }
    } catch (error) {
      return {
        command: null,
        label: null,
        error:
          error instanceof Error ? error.message : t("Check the chord values."),
      }
    }
  }
  const manual = preview(false)
  const generated = preview(true)

  async function insert(generatedChoice: boolean) {
    const result = preview(generatedChoice)
    if (!current || !result.command || submitting.current) return
    submitting.current = true
    setPending(true)
    setFailed(false)
    try {
      const inserted = await dispatch(result.command)
      if (inserted) {
        if (
          getProjectGeneration() === request.generation &&
          editor.context?.pattern.id === request.context.pattern.id &&
          editor.context?.channel === request.context.channel
        ) {
          editor.setSelection(inserted.created)
        }
        close()
      } else setFailed(true)
    } finally {
      submitting.current = false
      setPending(false)
    }
  }

  const numericFields = [
    {
      name: "root",
      label: t("Root / major-key tonic (MIDI)"),
      value: root,
      set: setRoot,
      min: 0,
      max: 127,
    },
    {
      name: "start",
      label: t("Start (ticks)"),
      value: start,
      set: setStart,
      min: 0,
      max: MAX_PATTERN_TICKS - 1,
    },
    {
      name: "length",
      label: t("Length (ticks)"),
      value: length,
      set: setLength,
      min: 1,
      max: MAX_PATTERN_TICKS,
    },
    {
      name: "seed",
      label: t("Seed integer"),
      value: seed,
      set: setSeed,
      min: MIN_SEED,
      max: MAX_SEED,
    },
  ]

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !submitting.current) close()
      }}
    >
      <DialogContent
        className="max-h-[85dvh] overflow-y-auto sm:max-w-xl"
        showCloseButton={!pending}
      >
        <DialogHeader>
          <DialogTitle>{t("Chord tools")}</DialogTitle>
          <DialogDescription>
            {t("Detect the selection or insert three simultaneous notes in one undo step.")}
          </DialogDescription>
        </DialogHeader>
        <p role="status" aria-label={t("Selection chord")}>
          {detection.label}
        </p>
        <FieldGroup>
          <Field>
            <FieldLabel>{t("Triad quality")}</FieldLabel>
            <ToggleGroup
              aria-label={t("Triad quality")}
              variant="outline"
              size="sm"
              className="flex-wrap"
              value={[quality]}
              disabled={pending}
              onValueChange={(values) => {
                const next = TRIADS.find((triad) => triad.quality === values[0])
                if (next) setQuality(next.quality)
              }}
            >
              {TRIADS.map((triad) => (
                <ToggleGroupItem key={triad.quality} value={triad.quality}>
                  {triad.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </Field>
          {numericFields.map((field) => {
            const value = whole(field.value)
            const invalid =
              !Number.isInteger(value) || value < field.min || value > field.max
            return (
              <Field
                key={field.name}
                data-invalid={invalid}
                data-disabled={pending}
              >
                <FieldLabel htmlFor={`${id}-${field.name}`}>
                  {field.label}
                </FieldLabel>
                <Input
                  id={`${id}-${field.name}`}
                  type="number"
                  min={field.min}
                  max={field.max}
                  step={1}
                  disabled={pending}
                  aria-invalid={invalid}
                  value={field.value}
                  onChange={(event) => field.set(event.target.value)}
                />
              </Field>
            )
          })}
          <FieldDescription>
            {t("Manual insertion uses the root and triad quality. The seed chooses among I, ii, iii, IV, V, vi and vii° in the major key whose tonic is above (MIDI 0–110). Next seed changes the choice; choices repeat every seven seeds.")}
          </FieldDescription>
        </FieldGroup>
        <p aria-label={t("Manual chord preview")}>{manual.label ?? manual.error}</p>
        <p aria-label={t("Seeded chord preview")}>
          {generated.label ?? generated.error}
        </p>
        {!current && (
          <Alert>
            <AlertDescription>
              {t("The project or piano-roll lane changed. Close and reopen chord tools.")}
            </AlertDescription>
          </Alert>
        )}
        {failed && (
          <Alert>
            <AlertDescription>
              {t("The chord could not be inserted. Check the current lane and try again.")}
            </AlertDescription>
          </Alert>
        )}
        <DialogFooter>
          <Button variant="ghost" disabled={pending} onClick={close}>
            {t("Close")}
          </Button>
          <Button
            variant="outline"
            disabled={
              pending ||
              !Number.isInteger(whole(seed)) ||
              whole(seed) < MIN_SEED ||
              whole(seed) > MAX_SEED
            }
            onClick={() => setSeed(String(advanceSeed(whole(seed))))}
          >
            {t("Next seed")}
          </Button>
          <Button
            variant="outline"
            disabled={pending || !current || !manual.command}
            onClick={() => void insert(false)}
          >
            {t("Insert triad")}
          </Button>
          <Button
            disabled={pending || !current || !generated.command}
            onClick={() => void insert(true)}
          >
            {pending ? t("Inserting…") : t("Insert seeded triad")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** One toolbar entry; all transient state stays inside the chord tool. */
export function ChordToolsControl() {
  const session = useSession()
  const [request, setRequest] = useState<Request | null>(null)
  useEffect(() => {
    const close = () => setRequest(null)
    const stopProject = onProjectReplaced(close)
    const stopHistory = onHistoryNavigation(close)
    return () => {
      stopProject()
      stopHistory()
    }
  }, [])
  return (
    <>
      <Button
        variant="outline"
        size="sm"
        onClick={() => {
          const context = session.editor.context
          if (!context || session.editor.busy) return
          const notes = session.editor.selectedNotes()
          setRequest({
            context,
            generation: getProjectGeneration(),
            revision: useProjectStore.getState().revision,
            root: notes.length
              ? Math.min(...notes.map((note) => note.key))
              : DEFAULT_KEY,
            start: notes.length
              ? Math.min(...notes.map((note) => note.start))
              : Math.max(
                  0,
                  Math.floor(
                    session.playhead ?? session.view?.viewport.scrollTick ?? 0
                  )
                ),
            length: usePianoRollStore.getState().lastLength,
          })
        }}
      >
        {t("Chords")}
      </Button>
      {request && (
        <ChordForm
          request={request}
          close={() =>
            setRequest((previous) => (previous === request ? null : previous))
          }
        />
      )}
    </>
  )
}
