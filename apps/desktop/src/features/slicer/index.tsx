import { useEffect, useId, useRef, useState } from "react"
import type { Clip } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import {
  getProjectGeneration,
  onProjectReplaced,
  useProjectGeneration,
} from "@/lib/store/replaced"
import { PPQ } from "@/lib/units"
import { usePlaylistStore } from "@/features/playlist/store"
import { nextSliceGrid } from "./grid-step"
import type { SliceOptions, SliceReview } from "./types"
import { translatedText, message, plainNumber } from "@/lib/i18n"

/** Entry point for one selected playlist clip; markers remain review-only. */
export function SliceControls({ clips }: { clips: Clip[] }) {
  const [session, setSession] = useState<{
    clip: Clip
    selection: ReadonlySet<number>
    generation: number
    ticket: number
  } | null>(null)
  const nextTicket = useRef(0)
  const single = clips.length === 1 && clips[0].content.type === "audio"
  useEffect(
    () =>
      usePlaylistStore.subscribe((state, previous) => {
        if (state.selection !== previous.selection)
          setSession((current) =>
            current?.generation === getProjectGeneration() ? null : current
          )
      }),
    []
  )
  return (
    <Dialog
      open={session !== null}
      onOpenChange={(open) => {
        if (!open) setSession(null)
        else if (single)
          setSession({
            clip: clips[0],
            selection: usePlaylistStore.getState().selection,
            generation: getProjectGeneration(),
            ticket: ++nextTicket.current,
          })
      }}
    >
      <DialogTrigger
        disabled={!single}
        render={<Button size="sm" variant="outline" />}
      >
        {translatedText("Slice clip")}
      </DialogTrigger>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{translatedText("Slice audio clip")}</DialogTitle>
          <DialogDescription>
            {translatedText("Review the cut markers, then split into clips linked to the original file. Apply is one undo step.")}
          </DialogDescription>
        </DialogHeader>
        {session && (
          <SliceForm
            key={session.ticket}
            clip={session.clip}
            selectionAtOpen={session.selection}
            generationAtOpen={session.generation}
            onApplied={() =>
              setSession((current) =>
                current?.ticket === session.ticket ? null : current
              )
            }
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function SliceForm({
  clip,
  selectionAtOpen,
  generationAtOpen,
  onApplied,
}: {
  clip: Clip
  selectionAtOpen: ReadonlySet<number>
  generationAtOpen: number
  onApplied: () => void
}) {
  const id = useId()
  const [mode, setMode] = useState<"grid" | "transients">("grid")
  const [grid, setGrid] = useState(PPQ)
  const [sensitivity, setSensitivity] = useState("50")
  const [review, setReview] = useState<SliceReview | null>(null)
  const [reviewRevision, setReviewRevision] = useState(-1)
  const [reviewGeneration, setReviewGeneration] = useState(-1)
  const [selected, setSelected] = useState<number[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const revision = useProjectStore((s) => s.revision)
  const generation = useProjectGeneration()
  const alive = useRef(false)
  const retained = useRef<SliceReview | null>(null)
  useEffect(() => {
    alive.current = true
    const offProject = onProjectReplaced(() => {
      if (retained.current)
        void backend.sliceDiscard(retained.current.token).catch(() => {})
      retained.current = null
      setBusy(false)
      setError(translatedText("The project changed. Analyze the clip again."))
    })
    return () => {
      alive.current = false
      offProject()
      if (retained.current) {
        void backend.sliceDiscard(retained.current.token).catch(() => {})
        retained.current = null
      }
    }
  }, [])
  const replaced = generationAtOpen !== generation
  const stale = replaced || (review !== null && reviewRevision !== revision)
  const invalidSensitivity =
    sensitivity.trim() === "" ||
    !Number.isFinite(Number(sensitivity)) ||
    Number(sensitivity) < 0 ||
    Number(sensitivity) > 100
  function reset() {
    if (retained.current)
      void backend.sliceDiscard(retained.current.token).catch(() => {})
    retained.current = null
    setReview(null)
    setSelected([])
    setError("")
  }
  async function analyze() {
    if (
      busy ||
      generationAtOpen !== getProjectGeneration() ||
      selectionAtOpen !== usePlaylistStore.getState().selection
    )
      return
    reset()
    const atRevision = useProjectStore.getState().revision
    const atGeneration = getProjectGeneration()
    const options: SliceOptions =
      mode === "grid"
        ? { mode, gridTicks: grid }
        : { mode, sensitivity: Number(sensitivity) / 100 }
    setBusy(true)
    try {
      const result = await backend.sliceAnalyze(clip.id, options)
      if (
        !alive.current ||
        selectionAtOpen !== usePlaylistStore.getState().selection ||
        atGeneration !== getProjectGeneration() ||
        atRevision !== useProjectStore.getState().revision
      ) {
        void backend.sliceDiscard(result.token).catch(() => {})
        if (alive.current)
          setError(translatedText("The project changed. Analyze the clip again."))
        return
      }
      retained.current = result
      setReview(result)
      setReviewRevision(atRevision)
      setReviewGeneration(atGeneration)
      setSelected(result.analysis.markers.map((m) => m.tick))
    } catch (failure) {
      if (
        alive.current &&
        atGeneration === getProjectGeneration() &&
        selectionAtOpen === usePlaylistStore.getState().selection
      )
        setError(errorMessage(failure))
    } finally {
      if (alive.current) setBusy(false)
    }
  }
  async function apply() {
    if (
      !review ||
      stale ||
      reviewGeneration !== getProjectGeneration() ||
      reviewRevision !== useProjectStore.getState().revision ||
      selectionAtOpen !== usePlaylistStore.getState().selection ||
      busy
    )
      return
    const atGeneration = getProjectGeneration()
    setBusy(true)
    setError("")
    try {
      const result = await backend.sliceApply(review.token, selected)
      if (
        !alive.current ||
        atGeneration !== getProjectGeneration() ||
        selectionAtOpen !== usePlaylistStore.getState().selection
      )
        return
      receivePatch(result.patch)
      retained.current = null
      usePlaylistStore.getState().select(result.created)
      onApplied()
    } catch (failure) {
      if (
        alive.current &&
        atGeneration === getProjectGeneration() &&
        selectionAtOpen === usePlaylistStore.getState().selection
      )
        setError(errorMessage(failure))
    } finally {
      if (alive.current) setBusy(false)
    }
  }
  function toggle(tick: number) {
    setSelected((current) =>
      current.includes(tick)
        ? current.filter((t) => t !== tick)
        : [...current, tick].sort((a, b) => a - b)
    )
  }
  return (
    <FieldGroup>
      {backend.kind === "mock" && (
        <FieldDescription>
          {translatedText("Browser demonstration: audio and waveforms are simulated. Native analysis uses the loaded file.")}
        </FieldDescription>
      )}
      <Field>
        <FieldLabel>{translatedText("Find markers")}</FieldLabel>
        <ToggleGroup
          value={[mode]}
          disabled={busy}
          onValueChange={(value) => {
            if (value[0] === "grid" || value[0] === "transients") {
              reset()
              setMode(value[0])
            }
          }}
        >
          <ToggleGroupItem value="grid">{translatedText("Beat grid")}</ToggleGroupItem>
          <ToggleGroupItem value="transients">{translatedText("Transients")}</ToggleGroupItem>
        </ToggleGroup>
      </Field>
      {mode === "grid" ? (
        <Field>
          <FieldLabel>{translatedText("Grid division")}</FieldLabel>
          <ToggleGroup
            value={[String(grid)]}
            disabled={busy}
            onValueChange={(value) => {
              if (value[0]) {
                reset()
                setGrid(Number(value[0]))
              }
            }}
          >
            {[PPQ / 4, PPQ / 2, PPQ, PPQ * 2, PPQ * 4].map((ticks, index) => (
              <ToggleGroupItem key={ticks} value={String(ticks)}>
                {["1/16", "1/8", "1/4", "1/2", translatedText("4 beats")][index]}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              aria-label={translatedText("Choose a finer slice grid")}
              disabled={busy || nextSliceGrid(grid, "finer") === null}
              onClick={() => {
                const next = nextSliceGrid(grid, "finer")
                if (next !== null) {
                  reset()
                  setGrid(next)
                }
              }}
            >
              {translatedText("Finer")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              aria-label={translatedText("Choose a coarser slice grid")}
              disabled={busy || nextSliceGrid(grid, "coarser") === null}
              onClick={() => {
                const next = nextSliceGrid(grid, "coarser")
                if (next !== null) {
                  reset()
                  setGrid(next)
                }
              }}
            >
              {translatedText("Coarser")}
            </Button>
          </div>
          <FieldDescription>
            {translatedText("Note divisions aligned to the song, measured in quarter-note beats.")}
          </FieldDescription>
        </Field>
      ) : (
        <Field data-invalid={invalidSensitivity}>
          <FieldLabel htmlFor={id + "sensitivity"}>{translatedText("Sensitivity (%)")}</FieldLabel>
          <Input
            id={id + "sensitivity"}
            type="number"
            min="0"
            max="100"
            step="1"
            value={sensitivity}
            aria-invalid={invalidSensitivity}
            disabled={busy}
            onChange={(event) => {
              reset()
              setSensitivity(event.target.value)
            }}
          />
          <FieldDescription>
            {translatedText("Higher values include weaker energy attacks; review before applying.")}
          </FieldDescription>
          {invalidSensitivity && (
            <FieldError>{translatedText("Enter a sensitivity from 0 to 100.")}</FieldError>
          )}
        </Field>
      )}
      <Button
        variant="outline"
        disabled={
          busy || replaced || (mode === "transients" && invalidSensitivity)
        }
        onClick={() => void analyze()}
      >
        {busy ? translatedText("Working…") : translatedText("Analyze markers")}
      </Button>
      <FieldDescription>
        {translatedText("Tape clips support trim, offset, reverse, pitch, gain, pan, mute, and routing. Fades, spectral stretch, and project swing must be removed before slicing.")}
      </FieldDescription>
      {review && (
        <>
          <MarkerPreview review={review} selected={selected} />
          <FieldSet disabled={busy || stale}>
            <FieldLegend>{translatedText("Cut markers (")}{selected.length}{" "}{translatedText("selected)")}</FieldLegend>
            <FieldDescription>
              {translatedText("Positions are relative to the visible clip. Turn off any cut you do not want.")}
            </FieldDescription>
            <div className="flex flex-wrap gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={busy || stale}
                onClick={() =>
                  setSelected(review.analysis.markers.map((m) => m.tick))
                }
              >
                {translatedText("Select all")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={busy || stale}
                onClick={() => setSelected([])}
              >
                {translatedText("Clear markers")}
              </Button>
            </div>
            <FieldGroup className="max-h-44 overflow-y-auto">
              {review.analysis.markers.map((marker) => (
                <Field key={marker.tick} orientation="horizontal">
                  <Checkbox
                    id={id + marker.tick}
                    checked={selected.includes(marker.tick)}
                    disabled={busy || stale}
                    onCheckedChange={() => toggle(marker.tick)}
                  />
                  <FieldLabel htmlFor={id + marker.tick}>
                    {translatedText("Cut at")}{" "}{(marker.tick / PPQ).toFixed(3)}{" "}{translatedText("beats (")}{marker.tick}{" "}
                    {translatedText("ticks)")}
                  </FieldLabel>
                </Field>
              ))}
            </FieldGroup>
            {!review.analysis.markers.length && (
              <FieldDescription>
                {translatedText("No interior markers found. Try another grid or sensitivity.")}
              </FieldDescription>
            )}
          </FieldSet>
          <p role="status">
            {selected.length
              ? message("{count} linked-source slices", { count: plainNumber(selected.length + 1) })
              : translatedText("No cuts selected")}
          </p>
        </>
      )}
      {(stale || error) && (
        <FieldError>
          {replaced
            ? translatedText("The project changed. Close and reopen slicing.")
            : stale
              ? translatedText("The project changed. Analyze the clip again.")
              : error}
        </FieldError>
      )}
      <Button
        disabled={busy || stale || !review || !selected.length}
        onClick={() => void apply()}
      >
        {translatedText("Apply slices")}
      </Button>
    </FieldGroup>
  )
}

function MarkerPreview({
  review,
  selected,
}: {
  review: SliceReview
  selected: number[]
}) {
  const height = 100
  const width = 640
  const peaks = review.analysis.peaks
  return (
    <svg
      role="img"
      aria-label={message("Slice marker preview: {count} cuts", { count: plainNumber(selected.length) })}
      viewBox={`0 0 ${width} ${height}`}
      className="h-28 w-full rounded-md border bg-muted"
    >
      <line
        x1={0}
        y1={height / 2}
        x2={width}
        y2={height / 2}
        className="stroke-muted-foreground"
        opacity={0.25}
      />
      {peaks.map((peak, index) => (
        <line
          key={index}
          x1={((index + 0.5) * width) / peaks.length}
          x2={((index + 0.5) * width) / peaks.length}
          y1={height / 2 - peak * 40}
          y2={height / 2 + peak * 40}
          className="stroke-muted-foreground"
          strokeWidth={3}
        />
      ))}
      {review.analysis.markers.map((marker) => (
        <line
          key={marker.tick}
          x1={(marker.tick / review.lengthTicks) * width}
          x2={(marker.tick / review.lengthTicks) * width}
          y1={0}
          y2={height}
          className="stroke-primary"
          strokeWidth={2}
          opacity={selected.includes(marker.tick) ? 1 : 0.2}
        />
      ))}
    </svg>
  )
}
