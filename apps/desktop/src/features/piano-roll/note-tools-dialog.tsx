import { useId, useRef, useState, useSyncExternalStore } from "react"

import type {
  ArpDirection,
  FlamPosition,
  RhythmMode,
  NoteEdge,
  NoteGroove,
  NoteTransform,
  NoteGridUnit,
} from "@/bindings"
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

import {
  applyNoteTool,
  closeNoteTools,
  NOTE_TOOLS,
  parseChopSteps,
  parseChordMap,
  requestIsCurrent,
  useNoteTools,
  type NoteTool,
  type ToolRequest,
} from "./note-tools"
import { currentSession } from "./session"

function NumberField({
  label,
  value,
  set,
  min,
  max,
  step = 1,
  help,
}: {
  label: string
  value: string
  set(value: string): void
  min: number
  max: number
  step?: number
  help?: string
}) {
  const id = useId()
  const n = value.trim() === "" ? NaN : Number(value)
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
        onChange={(e) => set(e.target.value)}
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
}: {
  label: string
  value: T
  set(value: T): void
  items: { value: T; label: string }[]
}) {
  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <ToggleGroup
        aria-label={label}
        value={[value]}
        variant="outline"
        size="sm"
        onValueChange={(values) => {
          const selected = items.find((item) => item.value === values[0])
          if (selected) set(selected.value)
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

const GROOVES: { value: NoteGroove; label: string }[] = [
  { value: "straight", label: "Straight" },
  { value: "swing", label: "Swing" },
  { value: "latePairs", label: "Late pairs" },
  { value: "pushFour", label: "Push four" },
]
const number = (value: string) => (value.trim() === "" ? NaN : Number(value))
const within = (n: number, min: number, max: number, integer = false) =>
  Number.isFinite(n) &&
  n >= min &&
  n <= max &&
  (!integer || Number.isInteger(n))

function ToolForm({ request }: { request: ToolRequest }) {
  const [tool, setTool] = useState<NoteTool>(request.tool)
  const [edge, setEdge] = useState<NoteEdge>(request.edge)
  const [groove, setGroove] = useState<NoteGroove>("straight")
  const [grid, setGrid] = useState(String(request.grid))
  const [gridUnit, setGridUnit] = useState<NoteGridUnit | "ticks">(request.musical.unit)
  const [divisor, setDivisor] = useState(String(request.musical.divisor))
  const [strength, setStrength] = useState("100")
  const [length, setLength] = useState("50")
  const [glide, setGlide] = useState("240")
  const [overlap, setOverlap] = useState("0")
  const [velocity, setVelocity] = useState("80")
  const [spacing, setSpacing] = useState("30")
  const [velocityStep, setVelocityStep] = useState("-5")
  const [direction, setDirection] = useState<"up" | "down">("up")
  const [low, setLow] = useState("48")
  const [high, setHigh] = useState("84")
  const [transpose, setTranspose] = useState("0")
  const [range, setRange] = useState<"clamp" | "octaves">("clamp")
  const [origin, setOrigin] = useState("0")
  const [period, setPeriod] = useState("960")
  const [pattern, setPattern] = useState("0, 240:50:100, 600:100:80")
  const [gate, setGate] = useState("80")
  const [octaves, setOctaves] = useState("1")
  const [arpDirection, setArpDirection] = useState<ArpDirection>("ascending")
  const [span, setSpan] = useState<"original" | "cycles">("original")
  const [repetitions, setRepetitions] = useState("2")
  const [flamPosition, setFlamPosition] = useState<FlamPosition>("before")
  const [flamVelocity, setFlamVelocity] = useState("50")
  const [interval, setInterval] = useState("30")
  const [rhythmMode, setRhythmMode] = useState<RhythmMode>("remove")
  const [cells, setCells] = useState("2")
  const [phase, setPhase] = useState("0")
  const [offset, setOffset] = useState("240")
  const [seed, setSeed] = useState("1")
  const [randomPitch, setRandomPitch] = useState("0")
  const [randomVelocity, setRandomVelocity] = useState("10")
  const [randomPan, setRandomPan] = useState("25")
  const [randomTiming, setRandomTiming] = useState("12")
  const [randomLength, setRandomLength] = useState("5")
  const [density, setDensity] = useState("75")
  const [root, setRoot] = useState("0")
  const [chordMap, setChordMap] = useState("0,4,7")
  const [velocityLow, setVelocityLow] = useState("55")
  const [velocityHigh, setVelocityHigh] = useState("95")
  const [pending, setPending] = useState(false)
  const applying = useRef(false)
  const [failure, setFailure] = useState<string | null>(null)
  const session = currentSession()
  useProjectStore((s) => s.revision)
  useSyncExternalStore(
    (notify) => session?.editor.subscribe(notify) ?? (() => {}),
    () => session?.editor.selection
  )
  const current = requestIsCurrent(request)
  const info = NOTE_TOOLS.find((item) => item.value === tool)!

  function transform(): NoteTransform | null {
    const g = number(grid)
    switch (tool) {
      case "lfo": return null
      case "quantize":
        return within(g, 1, 245760, true) && within(number(strength), 0, 100) && (gridUnit === "ticks" || within(number(divisor), 1, 96, true))
          ? {
              type: tool,
              grid: g,
              strength: number(strength) / 100,
              edge,
              groove,
              musical: gridUnit === "ticks" ? undefined : { ...request.musical, unit: gridUnit, divisor: number(divisor) },
            }
          : null
      case "staccato":
        return within(number(length), 1, 100)
          ? { type: tool, factor: number(length) / 100 }
          : null
      case "portamento":
        return within(number(glide), 1, 245760, true) && within(number(overlap), 0, 245760, true)
          ? { type: tool, glide: number(glide), overlap: number(overlap) }
          : null
      case "chop":
        return within(g, 1, 245760, true) ? { type: tool, grid: g } : null
      case "chopPattern": {
        const steps = parseChopSteps(pattern, number(period))
        return steps && within(number(origin), 0, 245760, true)
          ? {
              type: tool,
              origin: number(origin),
              period: number(period),
              steps,
            }
          : null
      }
      case "arpeggiate":
        return within(g, 1, 245760, true) &&
          within(number(gate), 0.1, 100) &&
          within(number(octaves), 1, 8, true) &&
          (span === "original" || within(number(repetitions), 1, 64, true))
          ? {
              type: tool,
              rate: g,
              gate: number(gate) / 100,
              octaves: number(octaves),
              repetitions: span === "original" ? 0 : number(repetitions),
              direction: arpDirection,
            }
          : null
      case "flam":
        return within(number(interval), 1, 960, true) &&
          within(number(flamVelocity), 0, 100)
          ? {
              type: tool,
              interval: number(interval),
              velocity: number(flamVelocity) / 100,
              position: flamPosition,
            }
          : null
      case "rhythmReshape":
        return within(number(origin), 0, 245760, true) &&
          within(g, 1, 245760, true) &&
          within(number(cells), 1, 64, true) &&
          within(number(phase), 0, number(cells) - 1, true) &&
          (rhythmMode === "remove" ||
            (within(number(offset), -245760, 245760, true) &&
              (rhythmMode !== "add" || number(offset) !== 0)))
          ? {
              type: tool,
              origin: number(origin),
              step: g,
              period: number(cells),
              phase: number(phase),
              offset: rhythmMode === "remove" ? 0 : number(offset),
              mode: rhythmMode,
            }
          : null
      case "strum":
        return within(number(spacing), 0, 245760, true) &&
          within(number(velocityStep), -100, 100)
          ? {
              type: tool,
              spacing: number(spacing),
              velocityStep: number(velocityStep) / 100,
              descending: direction === "down",
            }
          : null
      case "keyRange":
        return within(number(low), 0, 127, true) &&
          within(number(high), number(low), 127, true) &&
          within(number(transpose), -127, 127, true)
          ? {
              type: tool,
              low: number(low),
              high: number(high),
              transpose: number(transpose),
              octaves: range === "octaves",
            }
          : null
      case "scaleVelocity":
        return within(number(velocity), 0, 400)
          ? { type: tool, factor: number(velocity) / 100 }
          : null
      case "randomize":
        return within(number(seed), 0, 4294967295, true) &&
          within(number(randomPitch), 0, 127, true) &&
          within(number(randomVelocity), 0, 100) && within(number(randomPan), 0, 200) &&
          within(number(randomTiming), 0, 245760, true) && within(number(randomLength), 0, 100)
          ? { type: tool, seed: number(seed), pitch: number(randomPitch), velocity: number(randomVelocity) / 100,
              pan: number(randomPan) / 100, timing: number(randomTiming), length: number(randomLength) / 100 }
          : null
      case "generateRandom": {
        const pitchClasses = parseChordMap(chordMap)
        return pitchClasses !== null && within(number(seed), 0, 4294967295, true) &&
          within(g, 1, 245760, true) && within(number(density), 0, 100) && within(number(gate), 0.1, 100) &&
          within(number(root), 0, 11, true) && within(number(low), 0, 127, true) && within(number(high), number(low), 127, true) &&
          within(number(velocityLow), 0, 100) && within(number(velocityHigh), number(velocityLow), 100)
          ? { type: tool, seed: number(seed), grid: g, density: number(density) / 100, gate: number(gate) / 100,
              root: number(root), pitchClasses, low: number(low), high: number(high), velocityLow: number(velocityLow) / 100, velocityHigh: number(velocityHigh) / 100 }
          : null
      }
      default:
        return { type: tool }
    }
  }
  const options = transform()
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !applying.current) closeNoteTools()
      }}
    >
      <DialogContent
        showCloseButton={!pending}
        className="max-h-[90vh] overflow-y-auto sm:max-w-md"
        finalFocus={() => session?.focusGrid()}
      >
        <form
          noValidate
          onSubmit={async (event) => {
            event.preventDefault()
            if (!options || applying.current || !requestIsCurrent(request))
              return
            applying.current = true
            setPending(true)
            setFailure(null)
            try {
              const ok = await applyNoteTool(request, options)
              if (ok && useNoteTools.getState().request === request)
                closeNoteTools()
              else
                setFailure(
                  "Could not apply the tool. Check the notification, then reopen with a fresh selection."
                )
            } finally {
              applying.current = false
              setPending(false)
            }
          }}
          className="flex flex-col gap-4"
        >
          <DialogHeader>
            <DialogTitle>Selected-note tools</DialogTitle>
            <DialogDescription>
              {request.notes.length} selected notes. {info.description}
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="note-tool">Tool</FieldLabel>
              <Select
                items={NOTE_TOOLS}
                value={tool}
                onValueChange={(value) => {
                  const next = NOTE_TOOLS.find((item) => item.value === value)
                  if (next) {
                    setTool(next.value)
                    setFailure(null)
                  }
                }}
              >
                <SelectTrigger id="note-tool">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {NOTE_TOOLS.map((item) => (
                      <SelectItem key={item.value} value={item.value}>
                        {item.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            {((tool === "quantize" && gridUnit === "ticks") ||
              tool === "chop" ||
              tool === "arpeggiate" ||
              tool === "generateRandom" ||
              tool === "rhythmReshape") && (
              <NumberField
                label="Grid (ticks)"
                value={grid}
                set={setGrid}
                min={1}
                max={245760}
                help="240 = sixteenth note; 480 = eighth; 960 = quarter. Defaults to the current snap, or 240 when snap is off."
              />
            )}
            {(tool === "chopPattern" || tool === "rhythmReshape") && (
              <NumberField
                label="Origin (ticks)"
                value={origin}
                set={setOrigin}
                min={0}
                max={245760}
                help="The rhythm repeats before and after this tick. Tick zero remains the earliest legal note start."
              />
            )}
            {tool === "chopPattern" && (
              <>
                <NumberField
                  label="Period (ticks)"
                  value={period}
                  set={setPeriod}
                  min={1}
                  max={245760}
                />
                <Field data-invalid={!parseChopSteps(pattern, number(period))}>
                  <FieldLabel htmlFor="chop-pattern">
                    Pattern boundaries
                  </FieldLabel>
                  <Input
                    id="chop-pattern"
                    value={pattern}
                    maxLength={4096}
                    aria-invalid={!parseChopSteps(pattern, number(period))}
                    onChange={(e) => setPattern(e.target.value)}
                  />
                  <FieldDescription>
                    Enter 1–64 increasing ticks starting at 0, below Period,
                    separated by commas. Optional tick:gate%:velocity% (for
                    example 240:50:80). Gate must be greater than 0 through
                    100%; velocity 0–400%. Omitted accents are 100%. Gate scales
                    each retained piece, including partial edges, rounded to at
                    least one tick. Velocity multiplies the source and clamps at
                    full velocity.
                  </FieldDescription>
                </Field>
              </>
            )}
            {tool === "arpeggiate" && (
              <>
                <Choices<ArpDirection>
                  label="Arpeggio direction"
                  value={arpDirection}
                  set={setArpDirection}
                  items={[
                    { value: "ascending", label: "Ascending" },
                    { value: "descending", label: "Descending" },
                    { value: "alternating", label: "Alternating" },
                  ]}
                />
                <NumberField
                  label="Gate (%)"
                  value={gate}
                  set={setGate}
                  min={0.1}
                  max={100}
                  step={0.1}
                  help="Each hit is Grid × gate, rounded to at least one tick. No partial final slots are created."
                />
                <NumberField
                  label="Octave span"
                  value={octaves}
                  set={setOctaves}
                  min={1}
                  max={8}
                  help="1 uses the original pitches; 2 includes copies one octave higher. Any pitch above MIDI 127 refuses the whole edit."
                />
                <Choices<"original" | "cycles">
                  label="Duration"
                  value={span}
                  set={setSpan}
                  items={[
                    { value: "original", label: "Original span" },
                    { value: "cycles", label: "Fixed repetitions" },
                  ]}
                />
                {span === "cycles" && (
                  <NumberField
                    label="Repetitions"
                    value={repetitions}
                    set={setRepetitions}
                    min={1}
                    max={64}
                  />
                )}
                <FieldDescription>
                  Each exact-onset chord has its own rhythm. Voices sort by
                  pitch then ID across the octave span. Alternating goes up then
                  down without repeating the end voices. Original span uses the
                  longest source voice and must divide exactly by Grid. Voices
                  not reached within Original span are removed. Fixed
                  repetitions replaces that span with complete traversals (voice
                  count × Grid; alternating uses 2 × voice count − 2 hits, or
                  one for a single voice). This can extend the pattern; no time
                  or pitch is clipped.
                </FieldDescription>
              </>
            )}
            {tool === "flam" && (
              <>
                <Choices<FlamPosition>
                  label="Grace position"
                  value={flamPosition}
                  set={setFlamPosition}
                  items={[
                    { value: "before", label: "Before" },
                    { value: "after", label: "After" },
                  ]}
                />
                <NumberField
                  label="Flam interval (ticks)"
                  value={interval}
                  set={setInterval}
                  min={1}
                  max={960}
                  help="Grace start is this many ticks before or after the original. Grace length is the smaller of this interval and the original length."
                />
                <NumberField
                  label="Grace velocity (%)"
                  value={flamVelocity}
                  set={setFlamVelocity}
                  min={0}
                  max={100}
                  step={0.1}
                  help="Percentage of the original velocity. Pitch and pan stay the same."
                />
                <FieldDescription>
                  Before refuses any grace start below zero. After may overlap a
                  sustained original; it never shortens it. Any hit past tick
                  245760 refuses the entire edit.
                </FieldDescription>
              </>
            )}
            {tool === "rhythmReshape" && (
              <>
                <Choices<RhythmMode>
                  label="Rhythm mode"
                  value={rhythmMode}
                  set={setRhythmMode}
                  items={[
                    { value: "remove", label: "Remove" },
                    { value: "add", label: "Add" },
                    { value: "shift", label: "Shift" },
                  ]}
                />
                <NumberField
                  label="Period (steps)"
                  value={cells}
                  set={setCells}
                  min={1}
                  max={64}
                />
                <NumberField
                  label="Phase (step index)"
                  value={phase}
                  set={setPhase}
                  min={0}
                  max={Math.max(0, number(cells) - 1)}
                  help="0 targets the first onset cell in each period. A cell is Grid ticks wide, including its left edge and excluding its right edge."
                />
                {rhythmMode !== "remove" && (
                  <NumberField
                    label="Offset (ticks)"
                    value={offset}
                    set={setOffset}
                    min={-245760}
                    max={245760}
                    help="Positive moves later, negative earlier. Add needs a nonzero offset; Shift at zero is a no-op."
                  />
                )}
                <FieldDescription>
                  Onset cells repeat from Origin, including before it. Remove
                  can delete the entire selection. Add copies matched notes with
                  all properties; an identical selected hit at the destination
                  is skipped. Newly added hits join the selection, so later
                  Apply can add further hits. Shift moves matched originals with
                  lengths unchanged. Whole notes must fit tick 0–245760.
                </FieldDescription>
              </>
            )}
            {tool === "quantize" && (
              <>
                <Choices<NoteGridUnit | "ticks">
                  label="Grid unit"
                  value={gridUnit}
                  set={setGridUnit}
                  items={[{ value: "step", label: "Step" }, { value: "beat", label: "Beat" }, { value: "bar", label: "Bar" }, { value: "ticks", label: "Custom ticks" }]}
                />
                {gridUnit !== "ticks" && <NumberField label="Divisions per unit" value={divisor} set={setDivisor} min={1} max={96} help="Beat and bar grids follow each pattern meter; grooves restart at meter changes." />}
                <Choices<NoteEdge>
                  label="Quantize"
                  value={edge}
                  set={setEdge}
                  items={[
                    { value: "start", label: "Starts" },
                    { value: "end", label: "Ends" },
                  ]}
                />
                <NumberField
                  label="Strength (%)"
                  value={strength}
                  set={setStrength}
                  min={0}
                  max={100}
                  step={0.1}
                  help="100 reaches the grid; 50 moves halfway; 0 leaves timing alone."
                />
                <Choices<NoteGroove>
                  label="Groove"
                  value={groove}
                  set={setGroove}
                  items={GROOVES}
                />
                <FieldDescription>
                  Original rhythms: Swing delays alternate lines by 1/6 of a
                  grid; Late pairs by 1/4; Push four advances every fourth line
                  by 1/6.
                </FieldDescription>
              </>
            )}
            {tool === "staccato" && (
              <NumberField
                label="Length (%)"
                value={length}
                set={setLength}
                min={1}
                max={100}
                step={0.1}
              />
            )}
            {tool === "portamento" && (
              <>
                <NumberField
                  label="Glide (ticks)"
                  value={glide}
                  set={setGlide}
                  min={1}
                  max={245760}
                  step={1}
                />
                <NumberField
                  label="Overlap (ticks)"
                  value={overlap}
                  set={setOverlap}
                  min={0}
                  max={245760}
                  step={1}
                />
                <FieldDescription>
                  Each note reaches the next selected onset plus the overlap,
                  so a mono sound ties. The glide is how long every note after
                  the first takes to arrive from the pitch before it.
                </FieldDescription>
              </>
            )}
            {tool === "strum" && (
              <>
                <Choices<"up" | "down">
                  label="Direction"
                  value={direction}
                  set={setDirection}
                  items={[
                    { value: "up", label: "Low to high" },
                    { value: "down", label: "High to low" },
                  ]}
                />
                <NumberField
                  label="Delay per note (ticks)"
                  value={spacing}
                  set={setSpacing}
                  min={0}
                  max={245760}
                />
                <NumberField
                  label="Velocity step (%)"
                  value={velocityStep}
                  set={setVelocityStep}
                  min={-100}
                  max={100}
                  step={0.1}
                  help="Added for each successive note; -5 makes later notes softer."
                />
              </>
            )}
            {(tool === "keyRange" || tool === "generateRandom") && (
              <>
                <NumberField
                  label="Lowest MIDI key"
                  value={low}
                  set={setLow}
                  min={0}
                  max={127}
                  help="60 = C5 in Windfall."
                />
                <NumberField
                  label="Highest MIDI key"
                  value={high}
                  set={setHigh}
                  min={within(number(low), 0, 127) ? number(low) : 0}
                  max={127}
                />
                {tool === "keyRange" && <><NumberField
                  label="Transpose (semitones)"
                  value={transpose}
                  set={setTranspose}
                  min={-127}
                  max={127}
                />
                <Choices<"clamp" | "octaves">
                  label="Range handling"
                  value={range}
                  set={setRange}
                  items={[
                    { value: "clamp", label: "Clamp" },
                    { value: "octaves", label: "Fold octaves" },
                  ]}
                />
                </>}
              </>
            )}
            {tool === "scaleVelocity" && (
              <NumberField
                label="Velocity scale (%)"
                value={velocity}
                set={setVelocity}
                min={0}
                max={400}
                step={0.1}
                help="100 keeps velocities; 80 makes them 20% softer. Results clamp at full velocity."
              />
            )}
            {(tool === "randomize" || tool === "generateRandom") && <>
              <NumberField label="Seed" value={seed} set={setSeed} min={0} max={4294967295}
                help="The same seed and captured notes produce the same result." />
              <Button type="button" size="sm" variant="outline" onClick={() => setSeed(String((number(seed) + 1) >>> 0))}>Next seed</Button>
            </>}
            {tool === "randomize" && <>
              <NumberField label="Pitch variation (± semitones)" value={randomPitch} set={setRandomPitch} min={0} max={127} />
              <NumberField label="Velocity variation (± percentage points)" value={randomVelocity} set={setRandomVelocity} min={0} max={100} step={0.1} />
              <NumberField label="Pan variation (± percentage points)" value={randomPan} set={setRandomPan} min={0} max={200} step={0.1} />
              <NumberField label="Timing variation (± ticks)" value={randomTiming} set={setRandomTiming} min={0} max={245760} />
              <NumberField label="Length variation (± %)" value={randomLength} set={setRandomLength} min={0} max={100} step={0.1} />
              <FieldDescription>Zero keeps that property. Timing and lengths stay inside the pattern limit; pitch and levels clamp at their playable limits.</FieldDescription>
            </>}
            {tool === "generateRandom" && <>
              <NumberField label="Density (%)" value={density} set={setDensity} min={0} max={100} step={0.1} />
              <NumberField label="Gate (%)" value={gate} set={setGate} min={0.1} max={100} step={0.1} />
              <NumberField label="Root pitch class" value={root} set={setRoot} min={0} max={11} help="0 = C, 1 = C♯, …, 11 = B." />
              <Field data-invalid={parseChordMap(chordMap) === null}>
                <FieldLabel htmlFor="random-chord-map">Chord map (relative semitones)</FieldLabel>
                <Input id="random-chord-map" value={chordMap} maxLength={128} aria-invalid={parseChordMap(chordMap) === null} onChange={(event) => setChordMap(event.target.value)} />
                <FieldDescription>Comma-separated classes 0–11 above the root, repeated through the key range. 0,4,7 is a major triad; 0,2,4,5,7,9,11 is a major scale.</FieldDescription>
                <div className="flex flex-wrap gap-1">
                  {[{ label: "Major triad", map: "0,4,7" }, { label: "Minor triad", map: "0,3,7" }, { label: "Minor seventh", map: "0,3,7,10" }, { label: "Chromatic", map: "0,1,2,3,4,5,6,7,8,9,10,11" }].map((preset) => <Button key={preset.label} type="button" size="sm" variant="ghost" onClick={() => setChordMap(preset.map)}>{preset.label}</Button>)}
                </div>
              </Field>
              <NumberField label="Minimum velocity (%)" value={velocityLow} set={setVelocityLow} min={0} max={100} step={0.1} />
              <NumberField label="Maximum velocity (%)" value={velocityHigh} set={setVelocityHigh} min={number(velocityLow)} max={100} step={0.1} />
              <FieldDescription>The selection is replaced within its earliest start and latest end. Each grid cell may produce one note. Empty cells are rests; density zero clears the selection. A maximum of 16,384 cells keeps generation bounded.</FieldDescription>
            </>}
          </FieldGroup>
          {!current && (
            <Alert variant="destructive">
              <AlertDescription>
                The document or selection changed. Close this dialog and reopen
                the tool.
              </AlertDescription>
            </Alert>
          )}
          {failure && (
            <Alert variant="destructive">
              <AlertDescription>{failure}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={closeNoteTools}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={!options || !current || pending}>
              {pending ? "Applying…" : "Apply"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function NoteToolsDialog() {
  const request = useNoteTools((state) => state.request)
  return request ? (
    <ToolForm
      key={`${request.revision}:${request.pattern}:${request.channel}:${request.tool}:${request.edge}`}
      request={request}
    />
  ) : null
}
