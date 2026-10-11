import { useEffect, useRef, useState, type FormEvent } from "react"
import { toast } from "sonner"

import type {
  ExportOptions,
  ExportPlaylistStems,
  PlayMode,
} from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
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
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Progress,
  ProgressLabel,
  ProgressValue,
} from "@/components/ui/progress"
import { reportError } from "@/lib/errors"
import { projectName } from "@/lib/flows/project"
import { backend, errorMessage } from "@/lib/ipc"
import { useEngineStore } from "@/lib/store/engine"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

import { Choice, type Option } from "./choice"
import { useTimelineStore } from "@/features/playlist/timeline-store"
import { changeExportFormat, FormatControls } from "./format-controls"
import { PlaylistStemControls } from "./playlist-stem-controls"
import { StemControls } from "./stem-controls"
import { useMixerRender } from "./mixer-render"

import {
  cleanExportPath,
  exportedName,
  MAX_PATTERN_LOOPS,
  MAX_TAIL_SECS,
  parsePatternLoops,
  parseTailSecs,
  replaceExportExtension,
} from "./names"

const MODES: Option<PlayMode>[] = [
  { value: "pattern", label: "The selected pattern" },
  { value: "song", label: "The whole song" },
]

type Run = { fraction: number; error: string | null }

/** What the form holds. The two numbers stay text until they are sent. */
type Draft = Omit<ExportOptions, "patternLoops" | "tailSecs"> & {
  patternLoops: string
  tailSecs: string
}

const NO_PATH = "Choose where to save the file."
const BAD_LOOPS = `Enter a whole number from 1 to ${MAX_PATTERN_LOOPS}.`
const BAD_TAIL = `Enter a number of seconds from 0 to ${MAX_TAIL_SECS}.`

function ExportForm({ onDone }: { onDone(): void }) {
  const [mixerRequest] = useState(() => useMixerRender.getState().request)
  const exportTracks = useProjectStore((state) => state.project.mixer.tracks)
  const [draft, setDraft] = useState<Draft>(() => ({
    path: "",
    format: "wav",
    bitDepth: "int24",
    sampleRate: useEngineStore.getState().status?.sampleRate ?? 44_100,
    mode: useTimelineStore.getState().exportSelection
      ? "song"
      : useTransportStore.getState().mode,
    patternLoops: "4",
    tailSecs: "10",
    autoTail: true,
    stems: mixerRequest ? {
      mode: "trackOutputs",
      tracks: [...mixerRequest.tracks],
      includeMix: mixerRequest.includeMix,
      numbered: true,
      folder: true,
    } : undefined,
  }))
  // One file for each playlist track. It is a kind of export of its own:
  // it cannot be combined with mixer stems, and it needs the song.
  const [playlistStems, setPlaylistStems] = useState<
    ExportPlaylistStems | undefined
  >(undefined)
  const sentPlaylist = useRef(false)
  const [run, setRun] = useState<Run | null>(null)
  const [cancelling, setCancelling] = useState(false)
  const selectedRegion = useTimelineStore((state) => state.selection)
  const [selectedOnly, setSelectedOnly] = useState(
    () => useTimelineStore.getState().exportSelection
  )
  useEffect(() => {
    useTimelineStore.setState({ exportSelection: false })
    useMixerRender.setState({ request: null })
  }, [])
  // An empty path is only called out once Export was pressed without one.
  const [pathAsked, setPathAsked] = useState(false)
  const exporting = run !== null && run.error === null
  // The path the running export was started with, which is the name its
  // progress arrives under.
  const sent = useRef<ExportOptions | null>(null)

  const set = <K extends keyof Draft>(key: K, value: Draft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }))

  const path = cleanExportPath(draft.path)
  const tailSecs = parseTailSecs(draft.tailSecs)
  // Song mode does not use the count, so a bad one must not be in its way.
  const patternLoops =
    draft.mode === "song"
      ? (parsePatternLoops(draft.patternLoops) ?? 1)
      : parsePatternLoops(draft.patternLoops)
  const pathError = pathAsked && path === "" ? NO_PATH : null
  const loopsError = patternLoops === null ? BAD_LOOPS : null
  const tailError = tailSecs === null ? BAD_TAIL : null
  const stemsError =
    draft.stems?.tracks?.length === 0 && !draft.stems.includeMix
      ? "Choose at least one track or include the full mix."
      : null
  const mixerSourceError = mixerRequest && mixerRequest.generation !== getProjectGeneration()
    ? "The project was replaced. Close this export and choose its mixer tracks again."
    : draft.stems?.tracks?.some((id) => !exportTracks.some((track) => track.id === id && !track.current))
      ? "An export track was removed. Choose the remaining mixer tracks again."
      : null
  const playlistError = !playlistStems
    ? null
    : draft.mode !== "song"
      ? "Playlist tracks belong to the song. Render the whole song to export them."
      : playlistStems.tracks?.length === 0 && !playlistStems.includeMix
        ? "Choose at least one playlist track or include the full mix."
        : null
  const regionError =
    draft.mode === "song" && selectedOnly && !selectedRegion
      ? "The time selection was cleared. Select a region again or turn off region export."
      : null

  useEffect(
    () =>
      backend.onExportProgress((progress) => {
        const options = sent.current
        if (progress.path !== options?.path) return
        if (progress.error !== null) {
          setRun({ fraction: progress.fraction, error: progress.error })
        } else if (progress.done) {
          sent.current = null
          setCancelling(false)
          if (progress.cancelled) {
            setRun(null)
            toast("Export cancelled")
            return
          }
          toast.success("Exported", {
            description: sentPlaylist.current
              ? progress.files
                ? `${progress.files.length} files`
                : "Playlist tracks"
              : options.stems
                ? progress.files
                  ? `${progress.files.length} files`
                  : "Mixer stems"
                : exportedName(progress.path, options.format),
          })
          onDone()
        } else {
          setRun({ fraction: progress.fraction, error: null })
        }
      }),
    [onDone]
  )

  async function choosePath() {
    try {
      const picked = await backend.pickExportPath(projectName(), draft.format)
      if (picked !== null) set("path", picked)
    } catch (error) {
      reportError(error, "Could not choose a file")
    }
  }

  // The form checks itself: the browser's own validation would turn a
  // press on Export into nothing at all when a number is out of range.
  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const form = event.currentTarget
    const focus = (id: string) =>
      form.querySelector<HTMLElement>(`#${id}`)?.focus()
    if (path === "") {
      setPathAsked(true)
      focus("export-path")
      return
    }
    if (patternLoops === null) return focus("export-loops")
    if (tailSecs === null) return focus("export-tail")
    if (stemsError) return focus("export-stem-selection")
    if (mixerSourceError) return focus("export-stem-selection")
    if (playlistError) return focus("export-playlist-tracks")
    if (regionError) return

    const options: ExportOptions = {
      ...draft,
      path,
      patternLoops,
      tailSecs,
      region:
        draft.mode === "song" && selectedOnly && selectedRegion
          ? { ...selectedRegion }
          : undefined,
    }
    if (playlistStems) options.stems = undefined
    // The field shows the name the file gets.
    set("path", path)
    sent.current = options
    sentPlaylist.current = playlistStems !== undefined
    setCancelling(false)
    setRun({ fraction: 0, error: null })
    try {
      if (options.region) {
        const generation = getProjectGeneration()
        const revision = useProjectStore.getState().revision
        const source = await backend.timelineState()
        if (
          generation !== getProjectGeneration() ||
          revision !== useProjectStore.getState().revision ||
          revision !== source.revision
        )
          throw new Error(
            "The project changed before the selected region could be exported. Select the region again."
          )
        options.regionGeneration = source.generation
        options.regionRevision = source.revision
      }
      if (playlistStems)
        await backend.exportPlaylistTracks(options, { ...playlistStems })
      else await backend.exportAudio(options)
    } catch (error) {
      setRun({ fraction: 0, error: errorMessage(error) })
    }
  }

  async function cancelExport() {
    setCancelling(true)
    try {
      await backend.exportCancel()
    } catch (error) {
      setCancelling(false)
      reportError(error, "Could not cancel the export")
    }
  }

  return (
    <form noValidate onSubmit={onSubmit}>
      <FieldGroup>
        <FormatControls
          settings={draft}
          disabled={exporting}
          set={set}
          onFormat={(format) =>
            setDraft((current) => ({
              ...changeExportFormat(current, format),
              path: replaceExportExtension(current.path, format),
            }))
          }
        />
        <Field data-invalid={pathError !== null || undefined}>
          <FieldLabel htmlFor="export-path">Save to</FieldLabel>
          <div className="flex gap-2">
            <Input
              id="export-path"
              value={draft.path}
              placeholder={`Choose where to save the ${draft.format.toUpperCase()} file`}
              disabled={exporting}
              autoComplete="off"
              spellCheck={false}
              aria-invalid={pathError !== null || undefined}
              aria-describedby={pathError ? "export-path-error" : undefined}
              onChange={(event) => set("path", event.target.value)}
            />
            <Button
              type="button"
              variant="outline"
              disabled={exporting}
              onClick={() => void choosePath()}
            >
              Choose…
            </Button>
          </div>
          {pathError && (
            <FieldError id="export-path-error">{pathError}</FieldError>
          )}
        </Field>

        <FieldGroup className="grid grid-cols-2 gap-3">
          <Field orientation="horizontal" className="col-span-2">
            <Checkbox
              id="export-selected-region"
              checked={selectedOnly}
              disabled={
                exporting ||
                draft.mode !== "song" ||
                (!selectedRegion && !selectedOnly)
              }
              onCheckedChange={setSelectedOnly}
            />
            <FieldLabel htmlFor="export-selected-region">
              Export selected song region
              {selectedRegion
                ? ` (ticks ${selectedRegion.start}–${selectedRegion.end})`
                : " (select time in the playlist first)"}
            </FieldLabel>
          </Field>
          {regionError && (
            <FieldError className="col-span-2">{regionError}</FieldError>
          )}
          <Field>
            <FieldLabel htmlFor="export-mode">Render</FieldLabel>
            <Choice
              id="export-mode"
              value={draft.mode}
              options={MODES}
              disabled={exporting}
              onChange={(mode) => set("mode", mode)}
            />
          </Field>
          <Field data-invalid={loopsError !== null || undefined}>
            <FieldLabel htmlFor="export-loops">
              Times through the pattern
            </FieldLabel>
            <Input
              id="export-loops"
              type="number"
              min={1}
              max={MAX_PATTERN_LOOPS}
              value={draft.patternLoops}
              disabled={exporting || draft.mode === "song"}
              aria-invalid={loopsError !== null || undefined}
              aria-describedby={loopsError ? "export-loops-error" : undefined}
              onChange={(event) => set("patternLoops", event.target.value)}
            />
            {loopsError && (
              <FieldError id="export-loops-error">{loopsError}</FieldError>
            )}
          </Field>
          <Field orientation="horizontal" className="col-span-2">
            <Checkbox
              id="export-auto-tail"
              checked={draft.autoTail}
              disabled={exporting}
              onCheckedChange={(autoTail) => set("autoTail", autoTail)}
            />
            <FieldLabel htmlFor="export-auto-tail">
              Stop when the tail has faded
            </FieldLabel>
          </Field>
          <Field
            className="col-span-2"
            data-invalid={tailError !== null || undefined}
          >
            <FieldLabel htmlFor="export-tail">
              {draft.autoTail
                ? "Keep at most this many seconds after the end"
                : "Seconds to keep after the end, for tails to ring out"}
            </FieldLabel>
            <Input
              id="export-tail"
              type="number"
              min={0}
              max={MAX_TAIL_SECS}
              // What the arrows move by. Any number in the range is taken.
              step={0.5}
              value={draft.tailSecs}
              disabled={exporting}
              aria-invalid={tailError !== null || undefined}
              aria-describedby={tailError ? "export-tail-error" : undefined}
              onChange={(event) => set("tailSecs", event.target.value)}
            />
            {tailError && (
              <FieldError id="export-tail-error">{tailError}</FieldError>
            )}
          </Field>
        </FieldGroup>

        <StemControls
          value={draft.stems}
          disabled={exporting}
          onChange={(stems) => {
            set("stems", stems)
            if (stems) setPlaylistStems(undefined)
          }}
        />
        {stemsError && <FieldError>{stemsError}</FieldError>}
        {mixerSourceError && <FieldError>{mixerSourceError}</FieldError>}
        <PlaylistStemControls
          value={playlistStems}
          disabled={exporting}
          onChange={(next) => {
            setPlaylistStems(next)
            // Turning it on takes the place of mixer stems and needs the song.
            if (next && !playlistStems)
              setDraft((current) => ({
                ...current,
                stems: undefined,
                mode: "song",
              }))
          }}
        />
        {playlistError && <FieldError>{playlistError}</FieldError>}

        {exporting && (
          <Progress value={run.fraction * 100}>
            <ProgressLabel>Exporting</ProgressLabel>
            <ProgressValue />
          </Progress>
        )}
        {run?.error && <FieldError>The export failed. {run.error}</FieldError>}

        <DialogFooter>
          {exporting && (
            <Button
              type="button"
              variant="outline"
              disabled={cancelling}
              onClick={() => void cancelExport()}
            >
              {cancelling ? "Cancelling…" : "Cancel export"}
            </Button>
          )}
          <Button type="button" variant="outline" onClick={onDone}>
            {exporting ? "Close" : "Cancel"}
          </Button>
          <Button type="submit" disabled={exporting}>
            Export
          </Button>
        </DialogFooter>
      </FieldGroup>
    </form>
  )
}

/** Exports the pattern or song to audio files and shows progress. */
export function ExportDialog() {
  const open = useUiStore((state) => state.dialog === "export")
  const closeDialog = useUiStore((state) => state.closeDialog)

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeDialog()
      }}
    >
      <DialogContent className="max-h-[calc(100dvh-4rem)] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Export audio</DialogTitle>
          <DialogDescription>
            Save the selected pattern or song as a mix, as separate mixer
            tracks, or as one file for each playlist track.
          </DialogDescription>
        </DialogHeader>
        <ExportForm onDone={closeDialog} />
      </DialogContent>
    </Dialog>
  )
}
