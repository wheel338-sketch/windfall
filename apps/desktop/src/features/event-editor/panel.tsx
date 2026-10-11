import { useEffect, useId, useMemo, useRef, useState } from "react"

import type { AutomationPoint, AutomationTarget, Pattern } from "@/bindings"
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
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { openEventLfo } from "@/features/automation/lfo-dialog"
import {
  formatAutomationValue,
  formatBarBeat,
} from "@/features/playlist/automation/format"
import { curveValue, rangeNormalized } from "@/lib/automation/curve"
import { targetKey } from "@/lib/automation/lanes"
import { targetState } from "@/lib/automation/targets"
import { newGestureId } from "@/lib/store/gesture"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { usePlayhead } from "@/lib/store/realtime"
import { SNAP_OPTIONS, isSnapId, snapRound, snapTicks, type SnapId } from "@/features/piano-roll/snap"
import { patternTickInSong } from "@/features/piano-roll/view-math"
import { useTransportStore } from "@/lib/store/transport"
import { ticksPerBar, ticksPerBeat } from "@/lib/time"

import {
  closeEventEditor,
  showEventPattern,
  showEventTarget,
  shownPattern,
  useEventEditor,
} from "./actions"
import {
  MAX_EVENT_POINTS,
  changePoint,
  eventTargets,
  insertPoint,
  lanePoints,
  patternTicks,
  removePoint,
  samePoints,
} from "./model"

/** The curve's drawing area, in SVG units. */
const WIDTH = 720
const HEIGHT = 220
const PAD = 8
const SAMPLES = 240

const xOf = (tick: number, length: number) =>
  PAD + (tick / length) * (WIDTH - PAD * 2)
const yOf = (value: number) => PAD + (1 - value) * (HEIGHT - PAD * 2)

/**
 * The editor for one lane. It keeps a draft of the points while a drag or a
 * run of key presses is under way and sends each step to the document as
 * part of one gesture, so the whole move is a single undo step. A step is
 * sent with the points the document held before it; if something else
 * changed the lane meanwhile the command is refused and the draft is
 * dropped for what the project now has.
 */
function LaneEditor({
  pattern,
  target,
}: {
  pattern: Pattern
  target: AutomationTarget
}) {
  const project = useProjectStore((state) => state.project)
  const stored = useMemo(() => lanePoints(pattern, target), [pattern, target])
  const length = patternTicks(pattern)
  const state = targetState(project, target)
  const signature = pattern.timeSignature ?? project.settings.timeSignature
  const [snap, setSnap] = useState<SnapId>("none")
  const spacing = snapTicks(snap, signature)
  const playhead = useRef<SVGLineElement>(null)
  const mounted = useRef(true)
  const generation = useRef(getProjectGeneration())
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  usePlayhead((tick, playing) => {
    const line = playhead.current
    if (!line) return
    const transport = useTransportStore.getState()
    const project = useProjectStore.getState().project
    const local = transport.mode === "pattern"
      ? transport.pattern === pattern.id ? tick : null
      : patternTickInSong(tick, project.playlist.clips, project.playlist.tracks, pattern.id, length)
    const visible = playing && local !== null
    line.style.display = visible ? "" : "none"
    if (visible) { const x = String(xOf(local!, length)); line.setAttribute("x1", x); line.setAttribute("x2", x) }
  })

  const [draft, setDraft] = useState<AutomationPoint[] | null>(null)
  const [selected, setSelected] = useState<number | null>(null)
  const [refused, setRefused] = useState(false)
  const points = draft ?? stored
  // What the document is known to hold, for the next command's `expected`.
  const held = useRef(stored)
  const gesture = useRef<number | null>(null)
  const sending = useRef<Promise<unknown>>(Promise.resolve())
  const svg = useRef<SVGSVGElement | null>(null)
  const drag = useRef<number | null>(null)

  useEffect(() => {
    // The project moved on by itself (undo, another editor): follow it.
    if (!gesture.current) {
      held.current = stored
      setDraft(null)
    }
  }, [stored])

  /** Sends the points, one command after another so each sees the last. */
  const commit = (next: AutomationPoint[], id?: number) => {
    setDraft(next)
    sending.current = sending.current.then(async () => {
      if (!mounted.current || getProjectGeneration() !== generation.current) return
      const expected = held.current
      if (samePoints(expected, next)) return
      const result = await dispatch(
        {
          type: "setPatternEvents",
          pattern: pattern.id,
          target,
          expected,
          points: next,
        },
        id
      )
      if (result) {
        held.current = next
        setRefused(false)
      } else {
        // Refused: show what the project has and end the gesture.
        gesture.current = null
        setDraft(null)
        setRefused(true)
      }
    })
    return sending.current
  }

  const begin = () => {
    gesture.current = newGestureId()
  }
  const end = () => {
    const done = sending.current
    gesture.current = null
    void done.then(() => {
      if (!gesture.current) setDraft(null)
    })
  }

  /** The tick and value under a pointer event, in the lane's own units. */
  const place = (event: { clientX: number; clientY: number }) => {
    const box = svg.current?.getBoundingClientRect()
    if (!box || box.width === 0 || box.height === 0) return null
    const x = ((event.clientX - box.left) / box.width) * WIDTH
    const y = ((event.clientY - box.top) / box.height) * HEIGHT
    return {
      tick: Math.min(length, snapRound(Math.round(
        Math.min(1, Math.max(0, (x - PAD) / (WIDTH - PAD * 2))) * length
      ), spacing)),
      value: Math.min(1, Math.max(0, 1 - (y - PAD) / (HEIGHT - PAD * 2))),
    }
  }

  const add = (tick: number, value: number) => {
    if (points.length >= MAX_EVENT_POINTS) return
    const inserted = insertPoint(points, tick, value, length)
    setSelected(inserted.index)
    void commit(inserted.points)
  }

  const change = (index: number, patch: Partial<AutomationPoint>, id?: number) =>
    commit(changePoint(points, index, patch, length), id)

  const remove = (index: number) => {
    setSelected(null)
    void commit(removePoint(points, index))
  }

  const pathParts: string[] = []
  if (points.length) {
    for (let index = 0; index <= SAMPLES; index += 1) {
      const tick = (index / SAMPLES) * length
      pathParts.push(
        `${index ? "L" : "M"}${xOf(tick, length).toFixed(2)} ${yOf(curveValue(points, tick)).toFixed(2)}`
      )
    }
  }
  const path = pathParts.join(" ")

  const bar = ticksPerBar(signature)
  const beat = ticksPerBeat(signature)
  const grid: { tick: number; strong: boolean }[] = []
  for (let tick = 0; tick <= length && grid.length < 513; tick += beat) {
    grid.push({ tick, strong: tick % bar === 0 })
  }

  const current = selected !== null ? points[selected] : undefined
  const step = spacing || Math.max(1, Math.round(beat / 4))
  const describe = (point: AutomationPoint) =>
    `${formatBarBeat(point.tick, signature)}, ${formatAutomationValue(project, target, point.value) || `${Math.round(point.value * 100)}%`}`

  const tickId = useId()
  const valueId = useId()
  const curveId = useId()
  const holdId = useId()

  return (
    <div className="flex flex-col gap-3">
      <Field><FieldLabel>Snap</FieldLabel><Select items={SNAP_OPTIONS.map(({ id, label }) => ({ value: id, label }))} value={snap} onValueChange={(value) => { if (isSnapId(value)) setSnap(value) }}>
        <SelectTrigger aria-label="Event snap"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{SNAP_OPTIONS.map(({ id, label }) => <SelectItem key={id} value={id}>{label}</SelectItem>)}</SelectGroup></SelectContent>
      </Select></Field>
      {!state && (
        <Alert>
          <AlertDescription>
            The control this lane moved is no longer in the project. Clear the
            lane to remove it.
          </AlertDescription>
        </Alert>
      )}
      {refused && (
        <Alert>
          <AlertDescription>
            The lane changed while it was being edited, so the last change was
            not made. The curve shows what the project has now.
          </AlertDescription>
        </Alert>
      )}
      <svg
        ref={svg}
        role="application"
        aria-label="Event curve"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="h-56 w-full touch-none rounded border bg-display text-brand select-none"
        onDoubleClick={(event) => {
          const at = place(event)
          if (at && state) add(at.tick, at.value)
        }}
        onPointerMove={(event) => {
          const index = drag.current
          const at = place(event)
          if (index === null || !at || gesture.current === null) return
          void change(index, at, gesture.current)
        }}
        onPointerUp={() => {
          if (drag.current === null) return
          drag.current = null
          end()
        }}
        onPointerCancel={() => {
          if (drag.current === null) return
          drag.current = null
          end()
        }}
      >
        {grid.map((line) => (
          <line
            key={line.tick}
            x1={xOf(line.tick, length)}
            x2={xOf(line.tick, length)}
            y1={PAD}
            y2={HEIGHT - PAD}
            className={line.strong ? "stroke-border" : "stroke-border/40"}
            strokeWidth={line.strong ? 1 : 0.5}
          />
        ))}
        <line
          x1={PAD}
          x2={WIDTH - PAD}
          y1={yOf(0.5)}
          y2={yOf(0.5)}
          className="stroke-border/60"
          strokeDasharray="3 3"
        />
        {path && (
          <path
            d={path}
            fill="none"
            stroke="currentColor"
            strokeWidth={1.5}
            data-testid="event-curve"
          />
        )}
        <line ref={playhead} aria-label="Event playhead" x1={PAD} x2={PAD} y1={PAD} y2={HEIGHT - PAD} className="stroke-primary" strokeWidth={2} style={{ display: "none" }} pointerEvents="none" />
        {points.map((point, index) => (
          <circle
            key={index}
            role="slider"
            tabIndex={0}
            aria-label={`Point ${index + 1}: ${describe(point)}`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(point.value * 100)}
            aria-valuetext={describe(point)}
            cx={xOf(point.tick, length)}
            cy={yOf(point.value)}
            r={selected === index ? 5 : 4}
            className={
              selected === index
                ? "cursor-grab fill-foreground stroke-brand outline-none"
                : "cursor-grab fill-brand stroke-background outline-none"
            }
            strokeWidth={1.5}
            onFocus={() => setSelected(index)}
            onDoubleClick={(event) => event.stopPropagation()}
            onPointerDown={(event) => {
              if (event.button !== 0) return
              event.currentTarget.setPointerCapture?.(event.pointerId)
              setSelected(index)
              drag.current = index
              begin()
            }}
            onKeyDown={(event) => {
              const fine = event.shiftKey ? 0.1 : 1
              const moves: Record<string, Partial<AutomationPoint>> = {
                ArrowLeft: { tick: point.tick - Math.max(1, step * fine) },
                ArrowRight: { tick: point.tick + Math.max(1, step * fine) },
                ArrowUp: { value: point.value + 0.01 * fine },
                ArrowDown: { value: point.value - 0.01 * fine },
              }
              if (event.key === "Delete" || event.key === "Backspace") {
                event.preventDefault()
                remove(index)
              } else if (moves[event.key]) {
                event.preventDefault()
                // A held key is one undo step, like a drag.
                if (!gesture.current) begin()
                void change(index, moves[event.key], gesture.current ?? undefined)
              }
            }}
            onKeyUp={() => {
              if (drag.current === null && gesture.current) end()
            }}
          />
        ))}
      </svg>
      <p className="text-xs text-muted-foreground">
        {points.length === 0
          ? "This pattern has no events for this control. Double-click the curve area to add a point."
          : "Double-click to add a point. Drag a point or use the arrow keys to move it; Delete removes it."}
        {state &&
          ` Without a lane the control stays at ${formatAutomationValue(project, target, rangeNormalized(state.range, state.stored))}.`}
      </p>
      <FieldGroup className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Field>
          <FieldLabel htmlFor={tickId}>Tick</FieldLabel>
          <Input
            id={tickId}
            type="number"
            min={0}
            max={length}
            step={1}
            disabled={!current}
            value={current ? String(current.tick) : ""}
            onChange={(event) => {
              const tick = Number(event.target.value)
              if (selected !== null && Number.isFinite(tick))
                void change(selected, { tick })
            }}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={valueId}>Value (%)</FieldLabel>
          <Input
            id={valueId}
            type="number"
            min={0}
            max={100}
            step={0.1}
            disabled={!current}
            value={current ? String(Math.round(current.value * 1000) / 10) : ""}
            onChange={(event) => {
              const value = Number(event.target.value)
              if (selected !== null && Number.isFinite(value))
                void change(selected, { value: value / 100 })
            }}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={curveId}>Bend (%)</FieldLabel>
          <Input
            id={curveId}
            type="number"
            min={-100}
            max={100}
            step={1}
            disabled={!current}
            value={current ? String(Math.round(current.curve * 100)) : ""}
            onChange={(event) => {
              const curve = Number(event.target.value)
              if (selected !== null && Number.isFinite(curve))
                void change(selected, { curve: curve / 100 })
            }}
          />
        </Field>
        <div className="flex items-end justify-between gap-2 pb-1">
          <label htmlFor={holdId} className="text-sm">
            Hold until next
          </label>
          <Switch
            id={holdId}
            size="sm"
            disabled={!current}
            checked={current?.hold ?? false}
            onCheckedChange={(hold) => {
              if (selected !== null) void change(selected, { hold })
            }}
          />
        </div>
      </FieldGroup>
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={!state || points.length >= MAX_EVENT_POINTS}
          onClick={() => {
            if (!state) return
            // Where the pattern starts, at what the control has now.
            const value = points.length
              ? curveValue(points, 0)
              : rangeNormalized(state.range, state.stored)
            add(0, value)
          }}
        >
          Add point at start
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={selected === null}
          onClick={() => {
            if (selected !== null) remove(selected)
          }}
        >
          Remove point
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={!state}
          onClick={() => openEventLfo(pattern.id, target)}
        >
          Write LFO…
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={points.length === 0}
          onClick={() => {
            setSelected(null)
            void commit([])
          }}
        >
          Clear lane
        </Button>
      </div>
    </div>
  )
}

/** The event editor window. Mounted once; shows while it is open. */
export function EventEditorPanel() {
  const { open, pattern: chosen, target } = useEventEditor()
  const project = useProjectStore((state) => state.project)
  // Follows the transport's pattern until the editor is given its own.
  const selected = useTransportStore((state) => state.pattern)
  const patternId = open ? shownPattern(chosen ?? selected) : null
  const pattern = project.patterns.find((item) => item.id === patternId)
  const targets = useMemo(
    () => (open ? eventTargets(project, pattern, target) : []),
    [open, project, pattern, target]
  )
  const key = target ? targetKey(target) : ""
  const shown = targets.find((item) => item.key === key)
  const patternOptions = project.patterns.map((item) => ({
    value: String(item.id),
    label: item.name,
  }))
  const targetOptions = targets.map((item) => ({
    value: item.key,
    label: item.used ? `${item.label} (has events)` : item.label,
  }))

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeEventEditor()
      }}
    >
      <DialogContent className="max-h-[92dvh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Event editor</DialogTitle>
          <DialogDescription>
            Automation stored in a pattern. The control follows the curve
            wherever the pattern plays, by itself or as a clip, with no
            automation clip on the playlist.
          </DialogDescription>
        </DialogHeader>
        {open && pattern && (
          <div className="flex flex-col gap-4">
            <FieldGroup className="grid gap-3 sm:grid-cols-2">
              <Field>
                <FieldLabel>Pattern</FieldLabel>
                <Select
                  items={patternOptions}
                  value={String(pattern.id)}
                  onValueChange={(value) => {
                    const id = Number(value)
                    if (project.patterns.some((item) => item.id === id))
                      showEventPattern(id)
                  }}
                >
                  <SelectTrigger aria-label="Event pattern">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {patternOptions.map((item) => (
                        <SelectItem key={item.value} value={item.value}>
                          {item.label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              <Field>
                <FieldLabel>Control</FieldLabel>
                <Select
                  items={targetOptions}
                  value={shown ? shown.key : null}
                  onValueChange={(value) => {
                    const found = targets.find((item) => item.key === value)
                    if (found) showEventTarget(found.target)
                  }}
                >
                  <SelectTrigger aria-label="Event control">
                    <SelectValue placeholder="Choose a control" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {targetOptions.map((item) => (
                        <SelectItem key={item.value} value={item.value}>
                          {item.label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
            </FieldGroup>
            {shown ? (
              <LaneEditor
                key={`${pattern.id}:${shown.key}`}
                pattern={pattern}
                target={shown.target}
              />
            ) : (
              <p className="text-sm text-muted-foreground">
                Choose a control, or right-click any knob or fader and pick
                “Edit events in pattern”.
              </p>
            )}
          </div>
        )}
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={closeEventEditor}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
