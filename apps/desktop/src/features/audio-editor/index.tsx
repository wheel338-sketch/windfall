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
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import {
  getProjectGeneration,
  onProjectReplaced,
  useProjectGeneration,
} from "@/lib/store/replaced"
import { usePlaylistStore } from "@/features/playlist/store"
import { clockTime, validSelection, type Selection } from "./selection"
import { Choice, EditTools } from "./tools-panel"
import { AnalysisTools } from "./analysis-tools"
import { ScriptTools } from "./script-tools"
import type { AudioEditOperation, AudioEditPreview } from "./types"
import { Waveform } from "./waveform"
import { takeAudioEditorRequest, useAudioEditorRequest } from "./request"
import { Regions } from "./regions"
import { startSelectionDrag, endSelectionDrag, registerSelectionReceiver } from "./transfer"
import { translatedText as t, message } from "@/lib/i18n"

const OPERATIONS: {
  /** The edits without parameters, which travel as their name. */
  operation: Extract<AudioEditOperation, string>
  label: string
  help: string
}[] = [
  {
    operation: "trim",
    get label() { return t("Trim to selection") },
    get help() { return t("Keep the selection and replace the clip at its selected timeline position") },
  },
  {
    operation: "extract",
    get label() { return t("Extract selection") },
    get help() { return t("Add a copy after the original clip, leaving the original in place") },
  },
  {
    operation: "normalize",
    get label() { return t("Normalize") },
    get help() { return t("Normalize the selection to −1 dBFS with linked stereo gain") },
  },
  {
    operation: "reverse",
    get label() { return t("Reverse selection") },
    get help() { return t("Reverse the selected frames") },
  },
  {
    operation: "fadeIn",
    get label() { return t("Fade in selection") },
    get help() { return t("Apply an equal-power fade from silence to full level") },
  },
  {
    operation: "fadeOut",
    get label() { return t("Fade out selection") },
    get help() { return t("Apply an equal-power fade from full level to silence") },
  },
  {
    operation: "silence",
    get label() { return t("Silence selection") },
    get help() { return t("Replace the selection with silence, keeping its duration") },
  },
  {
    operation: "cut",
    get label() { return t("Cut selection") },
    get help() { return t("Remove the selection and join the remaining audio") },
  },
]

/**
 * Whether the playlist selection is still the one the editor opened with.
 * An edit replaces its clip, and the playlist drops the removed clip from
 * the selection when the replacement arrives, often before Apply's reply:
 * losing only clips that no longer exist keeps the editor's session, so it
 * can follow the edit to the derived clip.
 */
function selectionKept(atOpen: ReadonlySet<number>) {
  const current = usePlaylistStore.getState().selection
  if (current === atOpen) return true
  // Choosing the same clips again is still leaving the operation.
  const dropped = [...atOpen].filter((id) => !current.has(id))
  const clips = useProjectStore.getState().project.playlist.clips
  return dropped.length > 0 && [...current].every((id) => atOpen.has(id)) &&
    dropped.every((id) => !clips.some((clip) => clip.id === id))
}

export function AudioEditorButton({
  clip,
  disabled,
  defaultOpen = false,
  answersRequests = false,
}: {
  clip: number | null
  disabled?: boolean
  defaultOpen?: boolean
  /** Opens when the command palette asks for the editor. */
  answersRequests?: boolean
}) {
  // Capture the source at Open. The inspector may stop resolving that source
  // after Apply publishes its replacement, before the IPC reply arrives.
  const [session, setSession] = useState<{
    clip: number
    selection: ReadonlySet<number>
    generation: number
    ticket: number
    busy: boolean
  } | null>(() => defaultOpen && clip !== null ? {clip,selection:usePlaylistStore.getState().selection,generation:getProjectGeneration(),ticket:1,busy:false} : null)
  const nextTicket = useRef(defaultOpen ? 1 : 0)
  const selectingResult = useRef(false)
  const busy = session?.busy ?? false
  useEffect(() => {
    const offSelection = usePlaylistStore.subscribe((state, previous) => {
      if (state.selection !== previous.selection && !selectingResult.current)
        setSession((current) =>
          current?.busy && !useProjectStore.getState().project.playlist.clips.some(clip => clip.id === current.clip)
            ? current : current?.generation === getProjectGeneration() ? null : current
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
  const requested = useAudioEditorRequest((state) => state.pending)
  useEffect(() => {
    if (!answersRequests || !requested || disabled || clip === null) return
    if (!takeAudioEditorRequest()) return
    setSession((current) => current ?? {
      clip,
      selection: usePlaylistStore.getState().selection,
      generation: getProjectGeneration(),
      ticket: ++nextTicket.current,
      busy: false,
    })
  }, [answersRequests, requested, disabled, clip])
  function close() {
    setSession((current) =>
      current?.ticket === session?.ticket ? null : current
    )
  }
  return (
    <Dialog
      modal={false}
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
        {t("Audio editor")}
      </DialogTrigger>
      <DialogContent
        className="max-h-[90vh] overflow-y-auto sm:max-w-3xl"
        showCloseButton={!busy}
        showBackdrop={false}
      >
        <DialogHeader>
          <DialogTitle>{t("Audio editor")}</DialogTitle>
          <DialogDescription>
            {t("Select a range to process audio or edit its markers and notes. Each change is undoable. Original audio sources stay intact.")}
          </DialogDescription>
        </DialogHeader>
        {session && (
          <Editor
            key={session.ticket}
            clip={session.clip}
            selectionAtOpen={session.selection}
            onSelect={(id) => {
              selectingResult.current = true
              try { usePlaylistStore.getState().select([id]) }
              finally { selectingResult.current = false }
            }}
            onBusy={(busy) =>
              setSession((current) =>
                current?.ticket === session.ticket
                  ? { ...current, busy }
                  : current
              )
            }
            onDone={(clip) => setSession((current) => current?.ticket === session.ticket
              ? { ...current, clip, selection: usePlaylistStore.getState().selection, ticket: ++nextTicket.current, busy: false }
              : current)}
            onClose={close}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function Editor({
  clip,
  selectionAtOpen,
  onSelect,
  onBusy,
  onDone,
  onClose,
}: {
  clip: number
  selectionAtOpen: ReadonlySet<number>
  onSelect(id: number): void
  onBusy(value: boolean): void
  onDone(clip: number): void
  onClose(): void
}) {
  const [preview, setPreview] = useState<AudioEditPreview | null>(null)
  const [previewGeneration, setPreviewGeneration] = useState(-1)
  const [selection, setSelection] = useState<Selection>({ start: 0, end: 0 })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [spectral, setSpectral] = useState(false)
  const [saveDepth, setSaveDepth] = useState<"int16" | "int24">("int24")
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
      setError(t("The project changed. Close and reopen the audio editor."))
    })
    void backend
      .audioEditorOpen(clip)
      .then((value) => {
        if (
          cancelled ||
          atGeneration !== getProjectGeneration() ||
          !selectionKept(selectionAtOpen)
        ) {
          void backend.audioEditorDiscard(value.token).catch(() => {})
          if (!cancelled)
            setError(t("The project changed. Close and reopen the audio editor."))
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
          selectionKept(selectionAtOpen)
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
  useEffect(() => {
    if (!preview) return
    return registerSelectionReceiver(preview.token, {
      begin: () => {setBusy(true);onBusy(true);setError(null)},
      done: (target) => {if (alive.current) onDone(target ?? clip)},
      error: (reason) => {if (alive.current) {setBusy(false);onBusy(false);setError(errorMessage(reason))}},
    })
  }, [preview,clip,onBusy,onDone])
  async function apply(operation: AudioEditOperation) {
    if (!preview) return
    await commit(() => backend.audioEditorApply({ token:preview.token,operation,startFrame:selection.start,endFrame:selection.end }),true)
  }
  async function commit(work: () => ReturnType<typeof backend.audioEditorApply>, selectsCreated: boolean) {
    if (
      !preview ||
      stale ||
      previewGeneration !== getProjectGeneration() ||
      !selectionKept(selectionAtOpen) ||
      !validSelection(selection, preview.frames) ||
      busy
    )
      return
    const atGeneration = getProjectGeneration()
    setBusy(true)
    onBusy(true)
    setError(null)
    try {
      const result = await work()
      if (
        !alive.current ||
        atGeneration !== getProjectGeneration() ||
        !selectionKept(selectionAtOpen)
      )
        return
      receivePatch(result.patch)
      const id = selectsCreated ? result.created.at(-1) : preview.clip
      if (id !== undefined) onSelect(id)
      if (id !== undefined) onDone(id)
    } catch (error) {
      if (
        alive.current &&
        atGeneration === getProjectGeneration() &&
        selectionKept(selectionAtOpen)
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
  async function saveWavpack() {
    if (!preview || !valid || busy) return
    const savedToken = preview.token
    const savedGeneration = getProjectGeneration()
    setBusy(true); onBusy(true); setError(null)
    try {
      const path = await backend.pickExportPath(preview.name, "wv")
      if (!path) return
      if (savedGeneration !== getProjectGeneration() || selectionAtOpen !== usePlaylistStore.getState().selection)
        throw new Error(t("The project changed. Reopen the audio editor."))
      await backend.audioEditorSaveWavpack(savedToken, selection.start, selection.end, path, saveDepth)
    } catch (error) {
      if (alive.current) setError(errorMessage(error))
    } finally {
      if (alive.current) { setBusy(false); onBusy(false) }
    }
  }
  async function saveWav() {
    if (!preview || !valid || busy) return
    const savedToken = preview.token
    const savedGeneration = getProjectGeneration()
    setBusy(true); onBusy(true); setError(null)
    try {
      const path = await backend.pickExportPath(preview.name, "wav")
      if (!path) return
      if (!alive.current || savedGeneration !== getProjectGeneration() || selectionAtOpen !== usePlaylistStore.getState().selection)
        throw new Error(t("The project changed. Reopen the audio editor."))
      await backend.audioEditorSaveWav(savedToken, selection.start, selection.end, path)
    } catch (error) {
      if (alive.current && savedGeneration === getProjectGeneration()) setError(errorMessage(error))
    } finally {
      if (alive.current) { setBusy(false); onBusy(false) }
    }
  }
  return (
    <div className="flex flex-col gap-3">
      {(error || stale) && (
        <Alert variant="destructive">
          <AlertDescription>
            {stale
              ? t("The project changed. Close and reopen the audio editor.")
              : error}
          </AlertDescription>
        </Alert>
      )}
      {!preview && !error && <p role="status">{t("Preparing clip waveform…")}</p>}
      {preview && (
        <>
          <p className="text-muted-foreground">
            {preview.name} · {(preview.frames / preview.sampleRate).toFixed(3)}{" "}
            s · {preview.sampleRate}{" "}{t("Hz · stereo")}
          </p>
          <Waveform
            preview={preview}
            selection={selection}
            onSelection={setSelection}
            disabled={busy}
            spectral={spectral}
          />
          <Button size="sm" variant="outline" disabled={!preview.spectral?.length || busy} onClick={() => setSpectral(value => !value)}>{spectral ? t("Show waveform") : t("Show spectrum")}</Button>
          <FieldGroup className="flex-row items-end">
            <Field>
              <FieldLabel>{t("WavPack bit depth")}</FieldLabel>
              <Choice label={t("WavPack bit depth")} value={saveDepth} disabled={busy} onChange={setSaveDepth}
                options={[{value: "int16", label: t("16 bit")}, {value: "int24", label: t("24 bit")}]} />
            </Field>
            <Button size="sm" variant="outline" disabled={!valid || busy} onClick={() => void saveWavpack()}>{t("Save selection as WavPack")}</Button>
          </FieldGroup>
          {spectral && <p className="text-xs text-muted-foreground">{t("Time left to right · frequency 0–")}{preview.sampleRate/2}{" "}{t("Hz bottom to top · brightness −96–0 dBFS.")}</p>}
          <FieldGroup className="flex-row flex-wrap">
            <Field className="w-40" data-invalid={!valid}>
              <FieldLabel htmlFor="audio-edit-start">
                {t("Start frame (included)")}
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
              <ClipTime frame={selection.start} sampleRate={preview.sampleRate} />
            </Field>
            <Field className="w-40" data-invalid={!valid}>
              <FieldLabel htmlFor="audio-edit-end">
                {t("End frame (excluded)")}
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
              <ClipTime frame={selection.end} sampleRate={preview.sampleRate} />
            </Field>
          </FieldGroup>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => setSelection({ start: 0, end: preview.frames })}
            >
              {t("Select all")}
            </Button>
            <span className="text-muted-foreground">
              {valid
                ? message("{end} s selected", { end: ((selection.end - selection.start) / preview.sampleRate).toFixed(3) })
                : t("Choose a valid nonempty frame range")}
            </span>
          </div>
          <div
            role="group"
            aria-label={t("Audio edit operations")}
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
            key={preview.token}
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
                !selectionKept(selectionAtOpen)) return
              receivePatch(result.patch)
              const id = result.created.at(-1)
              if (id !== undefined) onSelect(id)
              onDone(id ?? clip)
            }}
          />
          <ScriptTools preview={preview} selection={selection} disabled={!valid || busy}
            onBusy={value => { setBusy(value); onBusy(value) }}
            onCommit={result => {
              if (!alive.current || previewGeneration !== getProjectGeneration() ||
                !selectionKept(selectionAtOpen)) return
              receivePatch(result.patch)
              const id = result.created.at(-1)
              if (id !== undefined) onSelect(id)
              onDone(id ?? clip)
            }} />
          <Regions preview={preview} selection={selection} disabled={!valid || busy} onSelect={setSelection} onWrite={regions=>void commit(()=>backend.audioEditorRegions(preview.token,regions),false)} />
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" disabled={!valid || busy} onClick={() => void saveWav()}>{t("Save selection as WAV with annotations")}</Button>
            <Button size="sm" variant="outline" disabled={!valid || busy || !preview.regions?.length}
              onClick={() => void commit(() => backend.audioEditorImportTimeline(preview.token), false)}>{t("Import annotations to timeline")}</Button>
          </div>
          <Button size="sm" variant="outline" disabled={!valid || busy} draggable={!!valid && !busy}
            onDragStart={event=>{if (!valid || busy) {event.preventDefault();return}; startSelectionDrag(event,{token:preview.token,startFrame:selection.start,endFrame:selection.end,durationSecs:(selection.end-selection.start)/preview.sampleRate})}}
            onDragEnd={endSelectionDrag}>{t("Drag selection to sampler or playlist")}</Button>
          <p className="text-muted-foreground">
            {t("Edits include the clip’s offset, reverse, pitch/stretch, gain, pan and fades. Mixer effects are excluded. Trim preserves the selected timeline position; extract places a new clip after the original. Cleaning and processing change only the selection; stretching, trimming silence and inserting silence move the audio after it. Maximum two minutes / 64 MiB per buffer.")}
          </p>
        </>
      )}
      {busy && <p role="status">{t("Processing audio…")}</p>}
      <div className="flex justify-end">
        <Button variant="outline" disabled={busy} onClick={onClose}>
          {t("Close editor")}
        </Button>
      </div>
    </div>
  )
}

/** A frame endpoint read as time, so a range relates to what is heard. */
function ClipTime({ frame, sampleRate }: { frame: number; sampleRate: number }) {
  if (!Number.isFinite(frame) || sampleRate <= 0) return null
  return (
    <FieldDescription className="tabular-nums">
      {message("{time} from the clip start", { time: clockTime(frame / sampleRate) })}
    </FieldDescription>
  )
}
