import { useState, type KeyboardEvent, type ReactNode } from "react"

import type { EffectParams } from "@/bindings"
import { formatMs, ToggleLed } from "@/components/audio"
import { Button } from "@/components/ui/button"
import {
  formatParam,
  ParamControl,
  ParamGroup,
  ParamRow,
  type ParamBinder,
} from "@/features/params"
import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"

import type { EditorProps } from "../editor-props"
import {
  chainSummary,
  linked,
  parallelEdits,
  serialEdits,
  UNIT_COUNT,
  unitParam,
  type RoutingEdit,
  type UnitRouting,
} from "./chain"

type EchoBankParams = Extract<EffectParams, { type: "echoBank" }>

const UNITS = Array.from({ length: UNIT_COUNT }, (_, unit) => unit)

/** The routing of every unit as the controls show it right now. */
function routing(bind: ParamBinder): UnitRouting[] {
  return UNITS.map((unit) => ({
    enabled: bind.value(unitParam(unit, "enabled")) >= 0.5,
    inputGain: bind.value(unitParam(unit, "inputGain")),
    outputGain: bind.value(unitParam(unit, "outputGain")),
    // The last unit has nothing after it; its stored send is always zero.
    nextSend: unit + 1 < UNIT_COUNT ? bind.value(unitParam(unit, "nextSend")) : 0,
  }))
}

type UnitTileProps = {
  unit: number
  selected: boolean
  routing: UnitRouting
  bind: ParamBinder
  onSelect(unit: number): void
}

/**
 * One unit in the chain: its on/off light, and a button that shows its
 * time and where it takes from and plays to, and picks it for the panel
 * below.
 */
function UnitTile({ unit, selected, routing, bind, onSelect }: UnitTileProps) {
  const enabled = bind(unitParam(unit, "enabled"))
  const synced = bind.value(unitParam(unit, "sync")) >= 0.5
  const division = bind.info(unitParam(unit, "division"))
  const time = synced
    ? formatParam(division, bind.value(unitParam(unit, "division")))
    : formatMs(bind.value(unitParam(unit, "timeMs")))
  const takesInput = routing.inputGain !== 0
  const plays = routing.outputGain !== 0
  const hint = useHint(
    `Unit ${unit + 1}: ${routing.enabled ? time : "off"}. ${takesInput ? "Takes the bank's input" : "Fed only by the unit before it"}, ${plays ? "heard at the output" : "not heard itself"}. Click to edit it`
  )
  return (
    <div
      data-slot="echo-unit"
      data-unit={unit}
      data-param={enabled.info.id}
      className="flex w-14 shrink-0 flex-col items-center gap-1"
    >
      <ToggleLed
        variant="dot"
        size="md"
        aria-label={enabled.info.name}
        pressed={routing.enabled}
        disabled={enabled.disabled}
        onPressedChange={(next) => {
          enabled.onGestureStart()
          enabled.onValueChange(next ? 1 : 0)
          enabled.onGestureEnd()
        }}
      />
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        aria-label={`Edit unit ${unit + 1}`}
        tabIndex={selected ? 0 : -1}
        data-unit-select={unit}
        onClick={() => onSelect(unit)}
        className={cn(
          "flex w-full flex-col items-center gap-0.5 rounded-md border border-border/80 bg-chassis/50 px-1 py-1 text-[10px] leading-none outline-none hover:border-foreground/40 focus-visible:ring-2 focus-visible:ring-ring",
          selected && "border-brand bg-background",
          !routing.enabled && "opacity-55"
        )}
        {...hint}
      >
        <span className="text-[11px] font-medium text-foreground">
          {unit + 1}
        </span>
        <span className="max-w-full truncate font-readout text-muted-foreground">
          {time}
        </span>
        <span aria-hidden className="flex gap-1 text-[8px] text-muted-foreground">
          <span className={cn(!takesInput && "opacity-25")}>IN</span>
          <span className={cn(!plays && "opacity-25")}>OUT</span>
        </span>
      </button>
    </div>
  )
}

type UnitChainProps = {
  selected: number
  onSelect(unit: number): void
  bind: ParamBinder
}

/**
 * All eight units in a row, with the send from each to the next between
 * them. This is where the bank is put in series, in parallel or anything
 * between: a send that is up feeds the unit after it, and an arrow lights
 * where sound really passes.
 */
function UnitChain({ selected, onSelect, bind }: UnitChainProps) {
  const units = routing(bind)

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    // Only the unit buttons walk with the arrows; a send's knob keeps its.
    const from = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-unit-select]"
    )
    if (!from) return
    let next: number
    if (event.key === "ArrowRight") next = (selected + 1) % UNIT_COUNT
    else if (event.key === "ArrowLeft") {
      next = (selected + UNIT_COUNT - 1) % UNIT_COUNT
    } else return
    event.preventDefault()
    onSelect(next)
    event.currentTarget
      .querySelector<HTMLElement>(`[data-unit-select="${next}"]`)
      ?.focus()
  }

  return (
    <div
      role="radiogroup"
      aria-label="Unit to edit"
      data-slot="echo-chain"
      onKeyDown={onKeyDown}
      className="flex min-w-0 items-end gap-1 overflow-x-auto pb-1"
    >
      {UNITS.map((unit) => (
        <div key={unit} className="flex shrink-0 items-end gap-1">
          <UnitTile
            unit={unit}
            selected={unit === selected}
            routing={units[unit]}
            bind={bind}
            onSelect={onSelect}
          />
          {unit + 1 < UNIT_COUNT ? (
            <div
              data-slot="echo-link"
              data-link={unit}
              data-linked={linked(units, unit) ? "" : undefined}
              className="flex shrink-0 flex-col items-center gap-0.5"
            >
              <ParamControl
                {...bind(unitParam(unit, "nextSend"))}
                size="sm"
                label={null}
                description={`How much of unit ${unit + 1} goes on into unit ${unit + 2}. Below zero it goes in inverted. At zero the two are side by side`}
              />
              <span
                aria-hidden
                className={cn(
                  "text-[11px] leading-none text-muted-foreground/40",
                  linked(units, unit) && "text-brand"
                )}
              >
                →
              </span>
            </div>
          ) : null}
        </div>
      ))}
    </div>
  )
}

type FilterGroupProps = {
  title: string
  /** `filter` or `feedbackFilter`. */
  field: string
  unit: number
  bind: ParamBinder
  about: string
  aside?: ReactNode
}

function FilterGroup({ title, field, unit, bind, about, aside }: FilterGroupProps) {
  const id = (name: string) => unitParam(unit, `${field}.${name}`)
  const off = bind.value(id("mode")) === 0
  return (
    <ParamGroup title={title} aside={aside}>
      <ParamControl
        {...bind(id("mode"))}
        label="Type"
        layout="inline"
        choiceStyle="select"
        description={about}
      />
      <ParamRow columns={3}>
        <ParamControl
          {...bind(id("frequencyHz"))}
          label="Freq"
          disabled={off || bind.disabled}
          description="Where the filter acts"
        />
        <ParamControl
          {...bind(id("q"))}
          label="Q"
          disabled={off || bind.disabled}
          description="How narrow or resonant the filter is"
        />
        <ParamControl
          {...bind(id("sections"))}
          label="Stages"
          disabled={off || bind.disabled}
          description="One to three copies of the filter in a row, for a steeper slope"
        />
        <ParamControl
          {...bind(id("gainDb"))}
          label="Boost"
          disabled={off || bind.disabled}
          description="Gain of a shelf or a peak. The other types ignore it"
        />
        <ParamControl
          {...bind(id("gain"))}
          label="Level"
          description="Level after the filter. It applies with the filter off too"
        />
      </ParamRow>
    </ParamGroup>
  )
}

type UnitPanelProps = {
  unit: number
  bind: ParamBinder
}

/** Every setting of the unit picked in the chain. */
function UnitPanel({ unit, bind }: UnitPanelProps) {
  const id = (field: string) => unitParam(unit, field)
  const synced = bind.value(id("sync")) >= 0.5
  const number = unit + 1
  return (
    <div
      role="group"
      aria-label={`Unit ${number}`}
      data-slot="echo-unit-panel"
      data-unit={unit}
      className="grid grid-cols-[repeat(auto-fit,minmax(11.5rem,1fr))] items-start gap-2"
    >
      <ParamGroup
        title={`Unit ${number} time`}
        aside={
          <ParamControl
            {...bind(id("sync"))}
            layout="inline"
            label="Sync"
            description="Follow the tempo with a note length, or set a time in milliseconds"
          />
        }
      >
        <ParamRow columns={2}>
          {synced ? (
            <ParamControl
              {...bind(id("division"))}
              label="Note"
              choiceStyle="select"
              className="w-24"
              description="The time between this unit's echoes as a note length"
            />
          ) : (
            <ParamControl
              {...bind(id("timeMs"))}
              label="Time"
              description="The time between this unit's echoes"
            />
          )}
          <ParamControl
            {...bind(id("stereoOffsetMs"))}
            label="Offset"
            description="Makes one side later than the other. Above zero delays the right, below zero the left"
          />
        </ParamRow>
      </ParamGroup>
      <ParamGroup title="In and out">
        <ParamRow columns={4}>
          <ParamControl
            {...bind(id("inputGain"))}
            label="Input"
            description="How much of the bank's input this unit takes. At zero it is fed only by the unit before it"
          />
          <ParamControl
            {...bind(id("inputPan"))}
            label="In pan"
            description="Balance of what the unit takes in"
          />
          <ParamControl
            {...bind(id("outputGain"))}
            label="Output"
            description="How much of the unit is heard. Below zero it is inverted. At zero it only feeds the next unit"
          />
          <ParamControl
            {...bind(id("outputPan"))}
            label="Out pan"
            description="Balance of the unit at the output"
          />
        </ParamRow>
      </ParamGroup>
      <ParamGroup title="Repeats">
        <ParamControl
          {...bind(id("feedbackMode"))}
          label="Mode"
          layout="inline"
          choiceStyle="select"
          description="Repeats keep their sides, change sides once, or bounce between them"
        />
        <ParamRow columns={3}>
          <ParamControl
            {...bind(id("feedback"))}
            label="Feedback"
            description="Share of each echo that makes the next one, times the bank's feedback"
          />
          <ParamControl
            {...bind(id("feedbackPan"))}
            label="Pan"
            description="Balance of the repeats"
          />
          <ParamControl
            {...bind(id("separation"))}
            label="Width"
            description="Stereo width of the unit, from mono to as it came in"
          />
        </ParamRow>
      </ParamGroup>
      <FilterGroup
        title="Filter"
        field="filter"
        unit={unit}
        bind={bind}
        about="Shapes everything the unit plays"
        aside={
          <ParamControl
            {...bind(id("filterPost"))}
            layout="inline"
            label="After delay"
            description="Filter what comes out of the delay instead of what goes in"
          />
        }
      />
      <FilterGroup
        title="Repeat filter"
        field="feedbackFilter"
        unit={unit}
        bind={bind}
        about="Shapes only what is fed back, so each repeat is filtered once more and the first echo is left clean"
      />
    </div>
  )
}

/**
 * The echo bank: eight delay units in a chain. The chain strip shows all
 * of them and the sends between them at once, which is where series,
 * parallel and mixed routings are made; the panel under it holds every
 * setting of the unit picked there.
 */
export function EchoBankEditor({ bind }: EditorProps<EchoBankParams>) {
  // Which unit the panel shows is a matter of the view and is not saved.
  const [unit, setUnit] = useState(0)
  const units = routing(bind)
  const group = bind.group("routing")

  // A routing button changes several units at once, as one undo step.
  function apply(edits: RoutingEdit[]) {
    if (edits.length === 0) return
    group.onGestureStart()
    for (const edit of edits) group.set(edit.id, edit.value)
    group.onGestureEnd()
  }

  return (
    <div
      data-slot="echo-bank-editor"
      role="group"
      aria-label="Echo bank settings"
      className="flex min-w-0 flex-col gap-2"
    >
      <ParamGroup
        title="Chain"
        aside={
          <div className="flex min-w-0 items-center gap-1.5">
            <span
              data-slot="echo-chain-summary"
              className="truncate font-readout text-[10px] text-muted-foreground"
            >
              <span className="sr-only">Routing: </span>
              {chainSummary(units)}
            </span>
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={bind.disabled}
              onClick={() => apply(serialEdits(units))}
            >
              Series
            </Button>
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={bind.disabled}
              onClick={() => apply(parallelEdits(units))}
            >
              Parallel
            </Button>
          </div>
        }
      >
        <UnitChain selected={unit} onSelect={setUnit} bind={bind} />
      </ParamGroup>
      <UnitPanel unit={unit} bind={bind} />
      <ParamGroup title="Bank">
        <ParamRow columns={4}>
          <ParamControl
            {...bind("dry")}
            description="Level of the sound as it came in"
          />
          <ParamControl
            {...bind("wet")}
            description="Level of all the units together"
          />
          <ParamControl
            {...bind("inputGain")}
            description="Level of what every unit takes in"
          />
          <ParamControl
            {...bind("feedback")}
            label="Feedback"
            description="Scales every unit's feedback at once"
          />
        </ParamRow>
      </ParamGroup>
    </div>
  )
}
