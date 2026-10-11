import { useEffect, useRef, useState } from "react"
import type { DispatchResult } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { backend, errorMessage } from "@/lib/ipc"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { usePlaylistStore } from "@/features/playlist/store"
import type { AudioEditPreview } from "./types"
import { selectionBetween, validSelection, type Selection } from "./selection"
import { Waveform } from "./waveform"
import type { AudioEditRecipe, AudioNoteOverride } from "./analysis-types"
import { mappedFrame, noteEdit, overrideNote, recipeProblem } from "./analysis-edit"

type Props = {
  preview: AudioEditPreview
  selection: Selection
  disabled: boolean
  onBusy(busy: boolean): void
  onCommit(result: DispatchResult): void
}
export function AnalysisTools({ preview, selection, disabled, onBusy, onCommit }: Props) {
  const [recipe, setRecipe] = useState<AudioEditRecipe | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [focus, setFocus] = useState(0)
  const [busy, setBusy] = useState(false)
  const [stale, setStale] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [undo, setUndo] = useState<AudioEditRecipe[]>([])
  const [redo, setRedo] = useState<AudioEditRecipe[]>([])
  const [sourcePeaks, setSourcePeaks] = useState<number[]>([])
  const [sourceSelection, setSourceSelection] = useState<Selection | null>(null)
  const alive = useRef(false)
  const working = useRef(false)
  const current = useRef<() => boolean>(() => false)
  useEffect(() => {
    alive.current = true
    const generation = getProjectGeneration()
    const revision = useProjectStore.getState().revision
    const selectionAtOpen = usePlaylistStore.getState().selection
    current.current = () => alive.current && generation === getProjectGeneration() &&
      revision === useProjectStore.getState().revision &&
      selectionAtOpen === usePlaylistStore.getState().selection
    const invalidate = () => {
      if (!current.current()) {
        setStale(true)
        void backend.audioAnalysisCancel().catch(() => {})
        void backend.previewStop().catch(() => {})
      }
    }
    const offProject = onProjectReplaced(invalidate)
    const offHistory = onHistoryNavigation(invalidate)
    const offSource = useProjectStore.subscribe(invalidate)
    const offSelection = usePlaylistStore.subscribe(invalidate)
    return () => {
      alive.current = false
      offProject(); offHistory(); offSource(); offSelection()
      void backend.audioAnalysisCancel().catch(() => {})
      void backend.previewStop().catch(() => {})
    }
  }, [preview.token])
  async function run(action: () => Promise<void>) {
    if (working.current || stale || !current.current()) return
    working.current = true
    setBusy(true); onBusy(true); setError(null)
    try { await action() } catch (e) {
      if (current.current()) setError(errorMessage(e))
    } finally {
      working.current = false
      if (alive.current) { setBusy(false); onBusy(false) }
    }
  }
  function change(next: AudioEditRecipe) {
    if (!recipe || busy || stale) return
    const problem = recipeProblem(next)
    if (problem) { setError(problem); return }
    setUndo((u) => [...u.slice(-63), recipe]); setRedo([])
    setRecipe(next); setError(null)
  }
  async function detect(polyphonic: boolean) {
    await run(async () => {
      const result = await backend.audioAnalysisDetect(preview.token, polyphonic)
      if (!current.current() || result.token !== preview.token) return
      setRecipe(result.recipe); setSelected(new Set(result.recipe.analysis.notes.map((_, i) => i)))
      setSourcePeaks(result.sourcePeaks ?? (result.recipe.analysis.frames === preview.frames ? preview.peaks : []))
      setSourceSelection(selectionBetween(selection.start, selection.end, result.recipe.analysis.frames))
      setFocus(0); setUndo([]); setRedo([])
    })
  }
  const problem = recipe && recipeProblem(recipe)
  const locked = disabled || busy || stale
  const edit = recipe?.analysis.notes[focus] ? noteEdit(recipe, focus) : null
  const range = sourceSelection ?? selection
  const rangeValid = !!recipe && validSelection(range, recipe.analysis.frames)
  function updateNote(patch: Partial<AudioNoteOverride>) {
    if (recipe && edit) change(overrideNote(recipe, { ...edit, ...patch }))
  }
  return (
    <section aria-label="Pitch, time and noise editors" className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={locked} onClick={() => void detect(false)}>Detect monophonic notes</Button>
        <Button variant="outline" disabled={locked} onClick={() => void detect(true)}>Detect spectral polyphony</Button>
      </div>
      <p className="text-muted-foreground text-xs">
        Monophonic pitch uses classical periodicity. Polyphony estimates at most eight simultaneous
        tonal peaks; harmonics, octave unisons, percussion and dense mixes can produce missing or
        extra notes. Review every score before insertion. No neural model is used.
      </p>
      {(error || problem || stale) && <Alert variant="destructive"><AlertDescription>
        {stale ? "Project or source changed. Reopen the editor." : error ?? problem}
      </AlertDescription></Alert>}
      {recipe && (
        <>
          <section aria-label="Preserved analysis source" className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">Preserved source waveform. Select source frames here for a noise profile or new warp marker.</p>
            <Waveform preview={{ ...preview, frames: recipe.analysis.frames,
              sampleRate: recipe.analysis.sampleRate, peaks: sourcePeaks }}
              selection={range} disabled={locked} onSelection={setSourceSelection} />
            <FieldGroup className="flex-row flex-wrap">
              <NumberField label="Analysis source selection start" value={range.start} disabled={locked}
                onChange={(start) => setSourceSelection({ ...range, start })} />
              <NumberField label="Analysis source selection end" value={range.end} disabled={locked}
                onChange={(end) => setSourceSelection({ ...range, end })} />
            </FieldGroup>
            {!rangeValid && <p role="alert">Choose an ordered source range inside the preserved waveform.</p>}
          </section>
          <Tabs defaultValue="pitch">
            <TabsList><TabsTrigger value="pitch">Pitch and score</TabsTrigger>
              <TabsTrigger value="time">Time and warp</TabsTrigger><TabsTrigger value="noise">Noise profile</TabsTrigger></TabsList>
            <TabsContent value="pitch" className="flex flex-col gap-2">
              <PitchCanvas recipe={recipe} focus={focus} selected={selected} disabled={locked || recipe.analysis.polyphonic}
                onFocus={setFocus} onEdit={(value) => change(overrideNote(recipe, value))} />
              {edit && !recipe.analysis.polyphonic && (
                <FieldGroup className="flex-row flex-wrap">
                  <NumberField label="Target MIDI pitch" value={edit.midiPitch} disabled={locked}
                    onChange={(v) => updateNote({ midiPitch: v })} step={0.01} />
                  <NumberField label="Note onset frame" value={edit.startFrame} disabled={locked}
                    onChange={(v) => updateNote({ startFrame: v })} />
                  <NumberField label="Note offset frame" value={edit.endFrame} disabled={locked}
                    onChange={(v) => updateNote({ endFrame: v })} />
                </FieldGroup>
              )}
              {edit && recipe.overrides.some((o) => o.note === focus) &&
                <Button variant="outline" disabled={locked} onClick={() => change({ ...recipe,
                  overrides: recipe.overrides.filter((o) => o.note !== focus) })}>Reset selected note to estimate</Button>}
              <div className="max-h-40 overflow-y-auto">
                {recipe.analysis.notes.map((note, i) => {
                  const value = noteEdit(recipe, i)
                  return <label key={i} className="flex items-center gap-2 text-xs">
                    <input type="checkbox" checked={selected.has(i)} disabled={locked} onChange={(e) => {
                      setSelected((s) => { const next = new Set(s); if (e.target.checked) next.add(i); else next.delete(i); return next })
                    }} />
                    Note {i + 1}: estimate {note.midiPitch.toFixed(2)} → {value.midiPitch.toFixed(2)}
                    {" · "}{(value.startFrame / recipe.analysis.sampleRate).toFixed(3)}–
                    {(value.endFrame / recipe.analysis.sampleRate).toFixed(3)} s
                    {" · "}velocity {Math.min(1, Math.sqrt(note.rms) * 1.6).toFixed(2)}
                  </label>
                })}
              </div>
              <Button disabled={locked || !!problem || selected.size === 0} onClick={() => void run(async () => {
                const result = await backend.audioAnalysisSendMidi({ token: preview.token, recipe }, [...selected])
                if (current.current()) onCommit(result)
              })}>Insert reviewed MIDI score</Button>
            </TabsContent>
            <TabsContent value="time" className="flex flex-col gap-2">
              <WarpCanvas recipe={recipe} disabled={locked} onChange={change} />
              <NumberField label="Output duration (frames)" value={recipe.markers.at(-1)!.targetFrame} disabled={locked}
                onChange={(v) => change({ ...recipe, markers: recipe.markers.map((m, i) =>
                  i === recipe.markers.length - 1 ? { ...m, targetFrame: v } : m) })} />
              <div className="max-h-40 overflow-y-auto">
                {recipe.markers.slice(1, -1).map((marker, i) => <FieldGroup key={marker.sourceFrame} className="flex-row items-end">
                  <NumberField label={`Transient ${i + 1} target frame (source ${marker.sourceFrame})`}
                    value={marker.targetFrame} disabled={locked} onChange={(v) => change({ ...recipe,
                      markers: recipe.markers.map((m, index) => index === i + 1 ? { ...m, targetFrame: v } : m) })} />
                  <Button variant="outline" disabled={locked} onClick={() => change({ ...recipe,
                    markers: recipe.markers.filter((_, index) => index !== i + 1) })}>Remove marker</Button>
                </FieldGroup>)}
              </div>
              <Button variant="outline" disabled={locked || !rangeValid || recipe.markers.length >= 512} onClick={() => {
                const sourceFrame = range.start
                if (sourceFrame <= 0 || sourceFrame >= recipe.analysis.frames ||
                  recipe.markers.some((m) => m.sourceFrame === sourceFrame)) return
                change({ ...recipe, markers: [...recipe.markers, { sourceFrame,
                  targetFrame: mappedFrame(recipe, sourceFrame) }].sort((a, b) => a.sourceFrame - b.sourceFrame) })
              }}>Add warp marker at selection start</Button>
            </TabsContent>
            <TabsContent value="noise" className="flex flex-col gap-2">
              <p className="text-xs text-muted-foreground">Select at least 45 ms of noise-only audio in the waveform, then learn its channel spectra.
                The profile is retained as a range into the preserved source. This reduces stationary noise; it is not a voice model or dereverb.</p>
              <Button variant="outline" disabled={locked || !rangeValid ||
                (range.end - range.start) / recipe.analysis.sampleRate < 0.045}
                onClick={() => change({ ...recipe, denoise: { profileStart: range.start,
                  profileEnd: range.end, reductionDb: 24, wet: 1 } })}>Learn noise profile from selection</Button>
              {recipe.denoise && <FieldGroup>
                <p>Profile frames {recipe.denoise.profileStart}–{recipe.denoise.profileEnd}</p>
                <NumberField label="Noise reduction (dB)" value={recipe.denoise.reductionDb} disabled={locked} step={1}
                  onChange={(v) => change({ ...recipe, denoise: { ...recipe.denoise!, reductionDb: v } })} />
                <NumberField label="Denoise wet mix" value={recipe.denoise.wet} disabled={locked} step={0.05}
                  onChange={(v) => change({ ...recipe, denoise: { ...recipe.denoise!, wet: v } })} />
                <Button variant="outline" disabled={locked} onClick={() => change({ ...recipe, denoise: null })}>Remove noise profile</Button>
              </FieldGroup>}
            </TabsContent>
          </Tabs>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={locked || !undo.length} onClick={() => {
              const previous = undo.at(-1)!; setUndo(undo.slice(0, -1)); setRedo([...redo, recipe]); setRecipe(previous)
            }}>Undo draft edit</Button>
            <Button variant="outline" disabled={locked || !redo.length} onClick={() => {
              const next = redo.at(-1)!; setRedo(redo.slice(0, -1)); setUndo([...undo, recipe]); setRecipe(next)
            }}>Redo draft edit</Button>
            <Button variant="outline" disabled={locked || !!problem} onClick={() => void run(() =>
              backend.audioAnalysisAudition({ token: preview.token, recipe }, true))}>Audition dry source</Button>
            <Button variant="outline" disabled={locked || !!problem} onClick={() => void run(() =>
              backend.audioAnalysisAudition({ token: preview.token, recipe }, false))}>Audition edited audio</Button>
            <Button variant="outline" onClick={() => void backend.previewStop()}>Stop audition</Button>
            <Button disabled={locked || !!problem || recipe.analysis.polyphonic} onClick={() => void run(async () => {
              const result = await backend.audioAnalysisApply({ token: preview.token, recipe })
              if (current.current()) onCommit(result)
            })}>Apply pitch, time and noise edits</Button>
          </div>
          <p className="text-xs text-muted-foreground">Dragging notes changes pitch and onset; numeric controls also change their offset.
            Arrow keys move a focused note by one semitone, or its onset by 10 ms with Shift.
            Apply preserves the source and stores the proposal with a derived WAV in one undo step.
            Reopening resumes saved edits. Spectral rendering has lookahead and approximate formants;
            interval boundaries and short notes need listening review.</p>
        </>
      )}
      {busy && <Button variant="outline" onClick={() => void backend.audioAnalysisCancel()}>Cancel analysis work</Button>}
    </section>
  )
}
function NumberField({ label, value, disabled, step = 1, onChange }: {
  label: string; value: number; disabled: boolean; step?: number; onChange(value: number): void
}) {
  return <Field><FieldLabel>{label}<Input aria-label={label} type="number" value={value}
    step={step} disabled={disabled} onChange={(e) => onChange(e.target.valueAsNumber)} /></FieldLabel></Field>
}
function PitchCanvas({ recipe, focus, selected, disabled, onFocus, onEdit }: {
  recipe: AudioEditRecipe; focus: number; selected: Set<number>; disabled: boolean
  onFocus(index: number): void; onEdit(edit: AudioNoteOverride): void
}) {
  const drag = useRef<{ x: number; y: number; width: number; height: number; edit: AudioNoteOverride } | null>(null)
  const duration = recipe.markers.at(-1)!.targetFrame
  const pitches = recipe.analysis.notes.map((_, i) => noteEdit(recipe, i).midiPitch)
  const low = Math.min(48, ...pitches) - 2, high = Math.max(84, ...pitches) + 2
  return <svg viewBox="0 0 800 220" role="group" aria-label="Detected note pitch editor"
    className="h-56 w-full rounded border bg-muted"
    onPointerUp={(e) => {
      const d = drag.current; drag.current = null
      if (!d || disabled) return
      const shift = Math.round((e.clientX - d.x) / d.width * duration)
      onEdit({ ...d.edit, midiPitch: Math.round((d.edit.midiPitch - (e.clientY - d.y) / d.height * (high - low)) * 100) / 100,
        startFrame: d.edit.startFrame + shift, endFrame: d.edit.endFrame + shift })
    }}
    onPointerCancel={() => { drag.current = null }}>
    {recipe.analysis.notes.map((n, i) => {
      const edit = noteEdit(recipe, i)
      return <rect key={i} role="button" tabIndex={0} aria-label={`Note ${i + 1} pitch ${edit.midiPitch.toFixed(2)}`}
        aria-pressed={focus === i} x={edit.startFrame / duration * 800}
        y={(high - edit.midiPitch) / (high - low) * 200}
        width={Math.max(3, (edit.endFrame - edit.startFrame) / duration * 800)} height={10}
        className={selected.has(i) ? "fill-primary stroke-foreground" : "fill-muted-foreground stroke-foreground"}
        onFocus={() => onFocus(i)} onClick={() => onFocus(i)}
        onPointerDown={(e) => {
          onFocus(i)
          if (disabled) return
          const svg = e.currentTarget.ownerSVGElement!
          const bounds = svg.getBoundingClientRect()
          drag.current = { x: e.clientX, y: e.clientY, width: bounds.width, height: bounds.height, edit }
          svg.setPointerCapture(e.pointerId)
        }}
        onKeyDown={(e) => {
          if (disabled || !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) return
          e.preventDefault()
          if (e.shiftKey) {
            const shift = Math.round(recipe.analysis.sampleRate / 100) * (e.key === "ArrowLeft" || e.key === "ArrowDown" ? -1 : 1)
            onEdit({ ...edit, startFrame: edit.startFrame + shift, endFrame: edit.endFrame + shift })
          } else onEdit({ ...edit, midiPitch: edit.midiPitch + (e.key === "ArrowDown" || e.key === "ArrowLeft" ? -1 : 1) })
        }}><title>Estimate {n.midiPitch.toFixed(2)}, confidence {n.confidence.toFixed(2)}</title></rect>
    })}
  </svg>
}
function WarpCanvas({ recipe, disabled, onChange }: {
  recipe: AudioEditRecipe; disabled: boolean; onChange(recipe: AudioEditRecipe): void
}) {
  const drag = useRef<number | null>(null)
  const frames = recipe.markers.at(-1)!.targetFrame
  return <svg viewBox="0 0 800 80" role="group" aria-label="Transient warp editor" className="h-20 w-full rounded border bg-muted"
    onPointerUp={(e) => {
      const index = drag.current; drag.current = null
      if (index === null || disabled) return
      const bounds = e.currentTarget.getBoundingClientRect()
      const targetFrame = Math.round((e.clientX - bounds.left) / bounds.width * frames)
      onChange({ ...recipe, markers: recipe.markers.map((m, i) => i === index ? { ...m, targetFrame } : m) })
    }} onPointerCancel={() => { drag.current = null }}>
    {recipe.markers.map((m, i) => <rect key={m.sourceFrame} role="slider" tabIndex={i === 0 ? -1 : 0}
      aria-label={i === recipe.markers.length - 1 ? "Warp duration endpoint" : `Warp marker ${i}`}
      aria-valuenow={m.targetFrame} aria-valuemin={0} aria-valuemax={frames * 4}
      x={m.targetFrame / frames * 790} y={10} width={8} height={60} className="fill-primary"
      onPointerDown={(e) => { if (disabled || i === 0) return; drag.current = i; e.currentTarget.ownerSVGElement!.setPointerCapture(e.pointerId) }}
      onKeyDown={(e) => {
        if (disabled || i === 0 || !["ArrowLeft", "ArrowRight"].includes(e.key)) return
        e.preventDefault()
        const targetFrame = m.targetFrame + (e.key === "ArrowLeft" ? -1 : 1) * Math.round(recipe.analysis.sampleRate / 100)
        onChange({ ...recipe, markers: recipe.markers.map((marker, index) => index === i ? { ...marker, targetFrame } : marker) })
      }} />)}
  </svg>
}
