import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"
import { toast } from "sonner"

import type { ExportOptions, ExportPlaylistStems, MasteringReview, PlayMode } from "@/bindings"
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
import {
  MasteringControls,
  MasteringReviewPanel,
} from "./mastering-controls"
import {
  DEFAULT_MASTERING_DRAFT,
  masteringRequest,
  type MasteringDraft,
} from "./mastering"
import { useMixerRender } from "./mixer-render"
import { beginExportRequest, failExportRequest, receiveExportRequest, setExportIntent, useExportRequestStore } from "./requests"
import { PluginExportControls } from "./plugin-export-controls"

import {
  cleanExportPath,
  exportedName,
  MAX_PATTERN_LOOPS,
  MAX_TAIL_SECS,
  parsePatternLoops,
  parseTailSecs,
  replaceExportExtension,
} from "./names"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const MODES: Option<PlayMode>[] = [
  { value: "pattern", get label() { return t("The selected pattern") } },
  { value: "song", get label() { return t("The whole song") } },
]

type Run = { fraction: number; error: string | null }

/** What the form holds. The two numbers stay text until they are sent. */
type Draft = Omit<ExportOptions, "patternLoops" | "tailSecs"> & {
  patternLoops: string
  tailSecs: string
}

const NO_PATH = () => t("Choose where to save the file.")
const BAD_LOOPS = () => message("Enter a whole number from 1 to {max}.", { max: plainNumber(MAX_PATTERN_LOOPS) })
const BAD_TAIL = () => message("Enter a number of seconds from 0 to {max}.", { max: plainNumber(MAX_TAIL_SECS) })

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
  const [master, setMaster] = useState<MasteringDraft>(DEFAULT_MASTERING_DRAFT)
  // What the last mastered export measured. The dialog stays open on it.
  const [review, setReview] = useState<MasteringReview | null>(null)
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
  const sentRequest = useRef<string | null>(null)
  const requestOwner = useRef<string | null>(null)

  const set = <K extends keyof Draft>(key: K, value: Draft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }))

  const path = cleanExportPath(draft.path)
  const tailSecs = parseTailSecs(draft.tailSecs)
  // Song mode does not use the count, so a bad one must not be in its way.
  const patternLoops =
    draft.mode === "song"
      ? (parsePatternLoops(draft.patternLoops) ?? 1)
      : parsePatternLoops(draft.patternLoops)
  const pathError = pathAsked && path === "" ? NO_PATH() : null
  const loopsError = patternLoops === null ? BAD_LOOPS() : null
  const tailError = tailSecs === null ? BAD_TAIL() : null
  const stemsError =
    draft.stems?.tracks?.length === 0 && !draft.stems.includeMix
      ? t("Choose at least one track or include the full mix.")
      : null
  const mixerSourceError = mixerRequest && mixerRequest.generation !== getProjectGeneration()
    ? t("The project was replaced. Close this export and choose its mixer tracks again.")
    : draft.stems?.tracks?.some((id) => !exportTracks.some((track) => track.id === id && !track.current))
      ? t("An export track was removed. Choose the remaining mixer tracks again.")
      : null
  const mastering = masteringRequest(master, draft.stems !== undefined || playlistStems !== undefined)
  const playlistError = !playlistStems
    ? null
    : draft.mode !== "song"
      ? t("Playlist tracks belong to the song. Render the whole song to export them.")
      : playlistStems.tracks?.length === 0 && !playlistStems.includeMix
        ? t("Choose at least one playlist track or include the full mix.")
        : null
  const regionError =
    draft.mode === "song" && selectedOnly && !selectedRegion
      ? t("The time selection was cleared. Select a region again or turn off region export.")
      : null

  useEffect(
    () =>
      backend.onExportProgress((progress) => {
        const options = sent.current
        if (progress.path !== options?.path || progress.request !== sentRequest.current) return
        receiveExportRequest(progress)
        if (progress.error !== null) {
          setRun({ fraction: progress.fraction, error: progress.error })
        } else if (progress.done) {
          sent.current = null
          setCancelling(false)
          if (progress.cancelled) {
            setRun(null)
            toast(t("Export cancelled"))
            return
          }
          toast.success(t("Exported"), {
            description: sentPlaylist.current
              ? progress.files
                ? message("{count} files", { count: plainNumber(progress.files.length) })
                : t("Playlist tracks")
              : options.stems
                ? progress.files
                  ? message("{count} files", { count: plainNumber(progress.files.length) })
                  : t("Mixer stems")
                : exportedName(progress.path, options.format),
          })
          if (progress.mastering) {
            // A master is reviewed before the dialog goes.
            setRun(null)
            setReview(progress.mastering)
            return
          }
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
      reportError(error, t("Could not choose a file"))
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
    if (master.enabled && !mastering.options) {
      return focus(
        mastering.targetError
          ? "export-mastering-target"
          : mastering.ceilingError
            ? "export-mastering-ceiling"
            : "export-mastering"
      )
    }

    const options: ExportOptions = {
      ...draft,
      path,
      patternLoops,
      tailSecs,
      mastering: mastering.options,
      region:
        draft.mode === "song" && selectedOnly && selectedRegion
          ? { ...selectedRegion }
          : undefined,
    }
    if (playlistStems) options.stems = undefined
    // The field shows the name the file gets.
    set("path", path)
    sent.current = options
    const intent = useExportRequestStore.getState().intent
    if (intent !== null) requestOwner.current = intent
    const request = beginExportRequest(path, requestOwner.current)
    sentRequest.current = request
    sentPlaylist.current = playlistStems !== undefined
    setCancelling(false)
    setReview(null)
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
            t("The project changed before the selected region could be exported. Select the region again.")
          )
        options.regionGeneration = source.generation
        options.regionRevision = source.revision
      }
      if (playlistStems)
        await backend.exportPlaylistTracks(options, { ...playlistStems }, request)
      else await backend.exportAudio(options, request)
    } catch (error) {
      failExportRequest(request, errorMessage(error))
      setRun({ fraction: 0, error: errorMessage(error) })
    }
  }

  async function cancelExport() {
    setCancelling(true)
    try {
      await backend.exportCancel()
    } catch (error) {
      setCancelling(false)
      reportError(error, t("Could not cancel the export"))
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
          <FieldLabel htmlFor="export-path">{t("Save to")}</FieldLabel>
          <div className="flex gap-2">
            <Input
              id="export-path"
              value={draft.path}
              placeholder={message("Choose where to save the {format} file", { format: draft.format.toUpperCase() })}
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
              {t("Choose…")}
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
              {t("Export selected song region")}
              {selectedRegion
                ? message(" (ticks {start}–{end})", { start: plainNumber(selectedRegion.start), end: plainNumber(selectedRegion.end) })
                : t(" (select time in the playlist first)")}
            </FieldLabel>
          </Field>
          {regionError && (
            <FieldError className="col-span-2">{regionError}</FieldError>
          )}
          <Field>
            <FieldLabel htmlFor="export-mode">{t("Render")}</FieldLabel>
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
              {t("Times through the pattern")}
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
              {t("Stop when the tail has faded")}
            </FieldLabel>
          </Field>
          <Field
            className="col-span-2"
            data-invalid={tailError !== null || undefined}
          >
            <FieldLabel htmlFor="export-tail">
              {draft.autoTail
                ? t("Keep at most this many seconds after the end")
                : t("Seconds to keep after the end, for tails to ring out")}
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

        <MasteringControls
          value={master}
          request={mastering}
          disabled={exporting}
          onChange={setMaster}
        />

        {exporting && (
          <Progress value={run.fraction * 100}>
            <ProgressLabel>
              {master.enabled ? t("Exporting and mastering") : t("Exporting")}
            </ProgressLabel>
            <ProgressValue />
          </Progress>
        )}
        {run?.error && <FieldError>{t("The export failed.")}{" "}{run.error}</FieldError>}
        {review && !exporting && <MasteringReviewPanel review={review} />}

        <DialogFooter>
          {exporting && (
            <Button
              type="button"
              variant="outline"
              disabled={cancelling}
              onClick={() => void cancelExport()}
            >
              {cancelling ? t("Cancelling…") : t("Cancel export")}
            </Button>
          )}
          <Button type="button" variant="outline" onClick={onDone}>
            {exporting ? t("Close") : review ? t("Done") : t("Cancel")}
          </Button>
          <Button type="submit" disabled={exporting}>
            {t("Export")}
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
  const close = useCallback(() => { setExportIntent(null); closeDialog() }, [closeDialog])

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) close()
      }}
    >
      <DialogContent className="max-h-[calc(100dvh-4rem)] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("Export audio")}</DialogTitle>
          <DialogDescription>
            {t("Save the selected pattern or song as a mix, as separate mixer tracks, or as one file for each playlist track.")}
          </DialogDescription>
        </DialogHeader>
        <ExportForm onDone={close} />
        <PluginExportControls />
      </DialogContent>
    </Dialog>
  )
}
