import { useEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import {
  getProjectGeneration,
  onProjectReplaced,
  useProjectGeneration,
} from "@/lib/store/replaced"
import { usePlaylistStore } from "@/features/playlist/store"
import { validSelection, type Selection } from "./selection"
import { EditTools } from "./tools-panel"
import { AnalysisTools } from "./analysis-tools"
import type { AudioEditOperation, AudioEditPreview } from "./types"
import { Waveform } from "./waveform"

const OPERATIONS: {
  /** The edits without parameters, which travel as their name. */
  operation: Extract<AudioEditOperation, string>
  label: string
  help: string
}[] = [
  {
    operation: "trim",
    label: "Trim to selection",
    help: "Keep the selection and replace the clip at its selected timeline position",
  },
  {
    operation: "extract",
    label: "Extract selection",
    help: "Add a copy after the original clip, leaving the original in place",
  },
  {
    operation: "normalize",
    label: "Normalize",
    help: "Normalize the selection to −1 dBFS with linked stereo gain",
  },
  {
    operation: "reverse",
    label: "Reverse selection",
    help: "Reverse the selected frames",
  },
  {
    operation: "fadeIn",
    label: "Fade in selection",
    help: "Apply an equal-power fade from silence to full level",
  },
  {
    operation: "fadeOut",
    label: "Fade out selection",
    help: "Apply an equal-power fade from full level to silence",
  },
  {
    operation: "silence",
    label: "Silence selection",
    help: "Replace the selection with silence, keeping its duration",
  },
  {
    operation: "cut",
    label: "Cut selection",
    help: "Remove the selection and join the remaining audio",
  },
]

export function AudioEditorButton({
  clip,
  disabled,
}: {
  clip: number | null
  disabled?: boolean
}) {
  // Capture the source at Open. The inspector may stop resolving that source
  // after Apply publishes its replacement, before the IPC reply arrives.
  const [session, setSession] = useState<{
    clip: number
    selection: ReadonlySet<number>
    generation: number
    ticket: number
    busy: boolean
  } | null>(null)
  const nextTicket = useRef(0)
  const busy = session?.busy ?? false
  useEffect(() => {
    const offSelection = usePlaylistStore.subscribe((state, previous) => {
      if (state.selection !== previous.selection)
        setSession((current) =>
          current?.generation === getProjectGeneration() ? null : current
        )
    })
    const offProject = onProjectReplaced(() =>
      setSession((current) => current && { ...current, busy: false })
    )
    return () => {
      offSelection()
      offProject()
    }
  }, [])
  function close() {
    setSession((current) =>
      current?.ticket === session?.ticket ? null : current
    )
  }
  return (
    <Dialog
      open={session !== null}
      onOpenChange={(value) => {
        if (busy) return
        if (!value) close()
        else if (clip !== null)
          setSession({
            clip,
            selection: usePlaylistStore.getState().selection,
            generation: getProjectGeneration(),
            ticket: ++nextTicket.current,
            busy: false,
          })
      }}
    >
      <DialogTrigger
        render={
          <Button
            variant="outline"
            size="sm"
            disabled={disabled || clip === null}
          />
        }
      >
        Audio editor
      </DialogTrigger>
      <DialogContent
        className="max-h-[90vh] overflow-y-auto sm:max-w-3xl"
        showCloseButton={!busy}
      >
        <DialogHeader>
          <DialogTitle>Audio editor</DialogTitle>
          <DialogDescription>
            Select a range, then apply an edit. Each edit creates a new WAV and
            one undo step. Original sources stay intact.
          </DialogDescription>
        </DialogHeader>
        {session && (
          <Editor
            key={session.ticket}
            clip={session.clip}
            selectionAtOpen={session.selection}
            onBusy={(busy) =>
              setSession((current) =>
                current?.ticket === session.ticket
                  ? { ...current, busy }
                  : current
              )
            }
            onDone={close}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function Editor({
  clip,
  selectionAtOpen,
  onBusy,
  onDone,
}: {
  clip: number
  selectionAtOpen: ReadonlySet<number>
  onBusy(value: boolean): void
  onDone(): void
}) {
  const [preview, setPreview] = useState<AudioEditPreview | null>(null)
  const [previewGeneration, setPreviewGeneration] = useState(-1)
  const [selection, setSelection] = useState<Selection>({ start: 0, end: 0 })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const token = useRef<number | null>(null)
  const alive = useRef(false)
  const generation = useProjectGeneration()
  useEffect(() => {
    let cancelled = false
    alive.current = true
    const atGeneration = getProjectGeneration()
    const offProject = onProjectReplaced(() => {
      if (token.current !== null) {
        void backend.audioEditorDiscard(token.current).catch(() => {})
        token.current = null
      }
      setBusy(false)
      setError("The project changed. Close and reopen the audio editor.")
    })
    void backend
      .audioEditorOpen(clip)
      .then((value) => {
        if (
          cancelled ||
          atGeneration !== getProjectGeneration() ||
          selectionAtOpen !== usePlaylistStore.getState().selection
        ) {
          void backend.audioEditorDiscard(value.token).catch(() => {})
          if (!cancelled)
            setError("The project changed. Close and reopen the audio editor.")
          return
        }
        token.current = value.token
        setPreview(value)
        setPreviewGeneration(atGeneration)
        setSelection({ start: 0, end: value.frames })
      })
      .catch((error: unknown) => {
        if (
          !cancelled &&
          atGeneration === getProjectGeneration() &&
          selectionAtOpen === usePlaylistStore.getState().selection
        )
          setError(errorMessage(error))
      })
    return () => {
      cancelled = true
      alive.current = false
      offProject()
      if (token.current !== null) {
        void backend.audioEditorDiscard(token.current).catch(() => {})
        token.current = null
      }
    }
  }, [clip, selectionAtOpen])

  const stale = preview !== null && previewGeneration !== generation
  async function apply(operation: AudioEditOperation) {
    if (
      !preview ||
      stale ||
      previewGeneration !== getProjectGeneration() ||
      selectionAtOpen !== usePlaylistStore.getState().selection ||
      !validSelection(selection, preview.frames) ||
      busy
    )
      return
    const atGeneration = getProjectGeneration()
    setBusy(true)
    onBusy(true)
    setError(null)
    try {
      const result = await backend.audioEditorApply({
        token: preview.token,
        operation,
        startFrame: selection.start,
        endFrame: selection.end,
      })
      if (
        !alive.current ||
        atGeneration !== getProjectGeneration() ||
        selectionAtOpen !== usePlaylistStore.getState().selection
      )
        return
      receivePatch(result.patch)
      const id = result.created.at(-1)
      if (id !== undefined) usePlaylistStore.getState().select([id])
      onDone()
    } catch (error) {
      if (
        alive.current &&
        atGeneration === getProjectGeneration() &&
        selectionAtOpen === usePlaylistStore.getState().selection
      )
        setError(errorMessage(error))
    } finally {
      if (alive.current) {
        setBusy(false)
        onBusy(false)
      }
    }
  }
  const valid = preview && !stale && validSelection(selection, preview.frames)
  return (
    <div className="flex flex-col gap-3">
      {(error || stale) && (
        <Alert variant="destructive">
          <AlertDescription>
            {stale
              ? "The project changed. Close and reopen the audio editor."
              : error}
          </AlertDescription>
        </Alert>
      )}
      {!preview && !error && <p role="status">Preparing clip waveform…</p>}
      {preview && (
        <>
          <p className="text-muted-foreground">
            {preview.name} · {(preview.frames / preview.sampleRate).toFixed(3)}{" "}
            s · {preview.sampleRate} Hz · stereo
          </p>
          <Waveform
            preview={preview}
            selection={selection}
            onSelection={setSelection}
            disabled={busy}
          />
          <FieldGroup className="flex-row flex-wrap">
            <Field className="w-40" data-invalid={!valid}>
              <FieldLabel htmlFor="audio-edit-start">
                Start frame (included)
              </FieldLabel>
              <Input
                id="audio-edit-start"
                type="number"
                step="1"
                min="0"
                max={preview.frames - 1}
                disabled={busy}
                aria-invalid={!valid}
                value={Number.isNaN(selection.start) ? "" : selection.start}
                onChange={(e) =>
                  setSelection({ ...selection, start: e.target.valueAsNumber })
                }
              />
            </Field>
            <Field className="w-40" data-invalid={!valid}>
              <FieldLabel htmlFor="audio-edit-end">
                End frame (excluded)
              </FieldLabel>
              <Input
                id="audio-edit-end"
                type="number"
                step="1"
                min="1"
                max={preview.frames}
                disabled={busy}
                aria-invalid={!valid}
                value={Number.isNaN(selection.end) ? "" : selection.end}
                onChange={(e) =>
                  setSelection({ ...selection, end: e.target.valueAsNumber })
                }
              />
            </Field>
          </FieldGroup>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => setSelection({ start: 0, end: preview.frames })}
            >
              Select all
            </Button>
            <span className="text-muted-foreground">
              {valid
                ? `${((selection.end - selection.start) / preview.sampleRate).toFixed(3)} s selected`
                : "Choose a valid nonempty frame range"}
            </span>
          </div>
          <div
            role="group"
            aria-label="Audio edit operations"
            className="flex flex-wrap gap-2"
          >
            {OPERATIONS.map(({ operation, label, help }) => (
              <Button
                key={operation}
                variant="outline"
                size="sm"
                title={help}
                disabled={
                  !valid ||
                  busy ||
                  (operation === "cut" &&
                    selection.start === 0 &&
                    selection.end === preview.frames)
                }
                onClick={() => void apply(operation)}
              >
                {label}
              </Button>
            ))}
          </div>
          <EditTools
            preview={preview}
            selection={selection}
            disabled={!valid || busy}
            onApply={(operation) => void apply(operation)}
          />
          <AnalysisTools
            preview={preview}
            selection={selection}
            disabled={!valid || busy}
            onBusy={(value) => { setBusy(value); onBusy(value) }}
            onCommit={(result) => {
              if (!alive.current || previewGeneration !== getProjectGeneration() ||
                selectionAtOpen !== usePlaylistStore.getState().selection) return
              receivePatch(result.patch)
              const id = result.created.at(-1)
              if (id !== undefined) usePlaylistStore.getState().select([id])
              onDone()
            }}
          />
          <p className="text-muted-foreground">
            Edits include the clip’s offset, reverse, pitch/stretch, gain, pan
            and fades. Mixer effects are excluded. Trim preserves the selected
            timeline position; extract places a new clip after the original.
            Cleaning and processing change only the selection; stretching,
            trimming silence and inserting silence move the audio after it.
            Maximum two minutes / 64 MiB per buffer.
          </p>
        </>
      )}
      {busy && <p role="status">Rendering and saving the edited audio…</p>}
      <div className="flex justify-end">
        <Button variant="outline" disabled={busy} onClick={onDone}>
          Close editor
        </Button>
      </div>
    </div>
  )
}
