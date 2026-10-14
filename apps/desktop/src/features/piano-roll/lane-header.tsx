import { logicalDelta } from "@/lib/ui-scale"
import { useRef, useSyncExternalStore } from "react"
import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { openNoteLfo } from "@/features/automation/lfo-dialog"

import { useSession } from "./context"
import { nextNoteFinePitchPreset } from "./fine-pitch-preset-step"
import { FINE_PITCH_PRESETS, finePitchPresetUpdates } from "./fine-pitch-presets"
import { finePitchScaleUpdates } from "./fine-pitch-scale"
import { nextLaneHeightScale } from "./lane-height-scale"
import { LANE_KINDS, type LaneKind } from "./lane-math"
import { modulationPresetStepUpdates } from "./modulation-preset-step"
import { MODULATION_PRESETS, modulationPresetUpdates } from "./modulation-presets"
import { modulationScaleUpdates } from "./modulation-scale"
import { nextNotePanPreset } from "./note-pan-preset-step"
import { NOTE_PAN_PRESETS, notePanPresetUpdates } from "./note-pan-presets"
import { notePanScaleUpdates } from "./note-pan-scale"
import { nextNoteReleasePreset } from "./release-preset-step"
import { RELEASE_PRESETS, releasePresetUpdates } from "./release-presets"
import { releaseScaleUpdates } from "./release-scale"
import { MAX_LANE_HEIGHT, MIN_LANE_HEIGHT, usePianoRollStore } from "./store"
import { nextNoteVelocityPreset } from "./velocity-preset-step"
import { VELOCITY_PRESETS, velocityPresetUpdates } from "./velocity-presets"
import { velocityScaleUpdates } from "./velocity-scale"
import { translatedText as t, message } from "@/lib/i18n"

const KEY_STEP_PX = 12

/** Picks what the lane under the grid shows: velocity or pan. */
export function LaneHeader() {
  const session = useSession()
  const kind = usePianoRollStore((state) => state.laneKind)
  const setKind = usePianoRollStore((state) => state.setLaneKind)
  const hint = useHint(t("What the bars under the notes show and edit"))
  const label = LANE_KINDS.find((item) => item.id === kind)?.label ?? kind
  const notesForPreset = () => {
    const selected = session.editor.selectedNotes()
    return selected.length > 0 ? selected : session.editor.notes
  }
  // Keep the menu current when selection, note expression, or lane height changes.
  useSyncExternalStore(
    (listener) => {
      const unsubscribeEditor = session.editor.subscribe(listener)
      const unsubscribeLayout = usePianoRollStore.subscribe(listener)
      return () => {
        unsubscribeEditor()
        unsubscribeLayout()
      }
    },
    () => {
      const context = session.editor.context
      const noteValues = notesForPreset()
        .map(
          ({ id, velocity, pan, expression }) =>
            `${id}:${velocity}:${pan}:${expression?.release ?? 0.5}:${expression?.finePitchCents ?? 0}:${expression?.modulationX ?? 0.5}:${expression?.modulationY ?? 0.5}`
        )
        .join(",")
      return `${context?.pattern.id}:${context?.channel}:${noteValues}:${usePianoRollStore.getState().laneHeight}`
    }
  )

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={message("Lane shows {label}", { label: label.toLowerCase() })}
        className="flex h-6 w-full items-center justify-between gap-0.5 px-1 text-[0.625rem] text-muted-foreground outline-none hover:bg-foreground/5 hover:text-foreground focus-visible:bg-foreground/10 focus-visible:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        {...hint}
      >
        <span className="truncate">{label}</span>
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="size-3 shrink-0"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-32">
        <DropdownMenuRadioGroup
          value={kind}
          onValueChange={(value: LaneKind) => {
            setKind(value)
            session.focusGrid()
          }}
        >
          {LANE_KINDS.map((item) => (
            <DropdownMenuRadioItem key={item.id} value={item.id} closeOnClick>
              {item.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        {kind === "velocity" && (
          <DropdownMenuGroup>
            {VELOCITY_PRESETS.map((preset) => (
              <DropdownMenuItem
                key={preset.label}
                disabled={
                  !session.editor.context ||
                  velocityPresetUpdates(notesForPreset(), preset.velocity)
                    .length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = velocityPresetUpdates(
                    notesForPreset(),
                    preset.velocity
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, velocity }) => ({
                      id,
                      patch: { velocity },
                    })),
                  })
                }}
              >
                {preset.label}
              </DropdownMenuItem>
            ))}
            {(["previous", "next"] as const).map((direction) => (
              <DropdownMenuItem
                key={direction}
                disabled={
                  !session.editor.context ||
                  notesForPreset().every(
                    (note) =>
                      nextNoteVelocityPreset(note.velocity, direction) === null
                  )
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = notesForPreset().flatMap((note) => {
                    const next = nextNoteVelocityPreset(note.velocity, direction)
                    return next === null
                      ? []
                      : [{ id: note.id, patch: { velocity: next } }]
                  })
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates,
                  })
                }}
              >
                {direction === "previous" ? t("Previous preset") : t("Next preset")}
              </DropdownMenuItem>
            ))}
            {(["half", "double"] as const).map((factor) => (
              <DropdownMenuItem
                key={factor}
                disabled={
                  !session.editor.context ||
                  velocityScaleUpdates(notesForPreset(), factor).length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = velocityScaleUpdates(notesForPreset(), factor)
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, velocity }) => ({
                      id,
                      patch: { velocity },
                    })),
                  })
                }}
              >
                {factor === "half" ? t("Half") : t("Double")}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        )}
        {kind === "pan" && (
          <DropdownMenuGroup>
            {NOTE_PAN_PRESETS.map((preset) => (
              <DropdownMenuItem
                key={preset.label}
                disabled={
                  !session.editor.context ||
                  notePanPresetUpdates(notesForPreset(), preset.pan).length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = notePanPresetUpdates(
                    notesForPreset(),
                    preset.pan
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, pan }) => ({
                      id,
                      patch: { pan },
                    })),
                  })
                }}
              >
                {preset.label}
              </DropdownMenuItem>
            ))}
            {(["previous", "next"] as const).map((direction) => (
              <DropdownMenuItem
                key={direction}
                disabled={
                  !session.editor.context ||
                  notesForPreset().every(
                    (note) => nextNotePanPreset(note.pan, direction) === null
                  )
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = notesForPreset().flatMap((note) => {
                    const next = nextNotePanPreset(note.pan, direction)
                    return next === null
                      ? []
                      : [{ id: note.id, patch: { pan: next } }]
                  })
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates,
                  })
                }}
              >
                {direction === "previous" ? t("Previous preset") : t("Next preset")}
              </DropdownMenuItem>
            ))}
            {(["half", "double"] as const).map((factor) => (
              <DropdownMenuItem
                key={factor}
                disabled={
                  !session.editor.context ||
                  notePanScaleUpdates(notesForPreset(), factor).length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = notePanScaleUpdates(notesForPreset(), factor)
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, pan }) => ({
                      id,
                      patch: { pan },
                    })),
                  })
                }}
              >
                {factor === "half" ? t("Half") : t("Double")}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        )}
        {kind === "release" && (
          <DropdownMenuGroup>
            {RELEASE_PRESETS.map((preset) => (
              <DropdownMenuItem
                key={preset.label}
                disabled={
                  !session.editor.context ||
                  releasePresetUpdates(notesForPreset(), preset.release)
                    .length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = releasePresetUpdates(
                    notesForPreset(),
                    preset.release
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {preset.label}
              </DropdownMenuItem>
            ))}
            {(["previous", "next"] as const).map((direction) => (
              <DropdownMenuItem
                key={direction}
                disabled={
                  !session.editor.context ||
                  notesForPreset().every(
                    (note) =>
                      nextNoteReleasePreset(
                        note.expression?.release ?? 0.5,
                        direction
                      ) === null
                  )
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = notesForPreset().flatMap((note) => {
                    const current = note.expression?.release ?? 0.5
                    const next = nextNoteReleasePreset(current, direction)
                    return next === null
                      ? []
                      : [
                          {
                            id: note.id,
                            patch: {
                              expression: {
                                ...(note.expression ?? DEFAULT_NOTE_EXPRESSION),
                                release: next,
                              },
                            },
                          },
                        ]
                  })
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates,
                  })
                }}
              >
                {direction === "previous" ? t("Previous preset") : t("Next preset")}
              </DropdownMenuItem>
            ))}
            {(["half", "double"] as const).map((factor) => (
              <DropdownMenuItem
                key={factor}
                disabled={
                  !session.editor.context ||
                  releaseScaleUpdates(notesForPreset(), factor).length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = releaseScaleUpdates(notesForPreset(), factor)
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {factor === "half" ? t("Half") : t("Double")}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        )}
        {kind === "finePitchCents" && (
          <DropdownMenuGroup>
            {FINE_PITCH_PRESETS.map((preset) => (
              <DropdownMenuItem
                key={preset.label}
                disabled={
                  !session.editor.context ||
                  finePitchPresetUpdates(notesForPreset(), preset.finePitchCents)
                    .length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = finePitchPresetUpdates(
                    notesForPreset(),
                    preset.finePitchCents
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {preset.label}
              </DropdownMenuItem>
            ))}
            {(["previous", "next"] as const).map((direction) => (
              <DropdownMenuItem
                key={direction}
                disabled={
                  !session.editor.context ||
                  notesForPreset().every(
                    (note) =>
                      nextNoteFinePitchPreset(
                        note.expression?.finePitchCents ?? 0,
                        direction
                      ) === null
                  )
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = notesForPreset().flatMap((note) => {
                    const current = note.expression?.finePitchCents ?? 0
                    const next = nextNoteFinePitchPreset(current, direction)
                    return next === null
                      ? []
                      : [
                          {
                            id: note.id,
                            patch: {
                              expression: {
                                ...(note.expression ?? DEFAULT_NOTE_EXPRESSION),
                                finePitchCents: next,
                              },
                            },
                          },
                        ]
                  })
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates,
                  })
                }}
              >
                {direction === "previous" ? t("Previous preset") : t("Next preset")}
              </DropdownMenuItem>
            ))}
            {(["half", "double"] as const).map((factor) => (
              <DropdownMenuItem
                key={factor}
                disabled={
                  !session.editor.context ||
                  finePitchScaleUpdates(notesForPreset(), factor).length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = finePitchScaleUpdates(notesForPreset(), factor)
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {factor === "half" ? t("Half") : t("Double")}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        )}
        {(kind === "modulationX" || kind === "modulationY") && (
          <DropdownMenuGroup>
            {MODULATION_PRESETS.map((preset) => (
              <DropdownMenuItem
                key={preset.label}
                disabled={
                  !session.editor.context ||
                  modulationPresetUpdates(
                    notesForPreset(),
                    kind,
                    preset.modulation
                  ).length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = modulationPresetUpdates(
                    notesForPreset(),
                    kind,
                    preset.modulation
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {preset.label}
              </DropdownMenuItem>
            ))}
            {(["previous", "next"] as const).map((direction) => (
              <DropdownMenuItem
                key={direction}
                disabled={
                  !session.editor.context ||
                  modulationPresetStepUpdates(notesForPreset(), kind, direction)
                    .length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = modulationPresetStepUpdates(
                    notesForPreset(),
                    kind,
                    direction
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {direction === "previous" ? t("Previous preset") : t("Next preset")}
              </DropdownMenuItem>
            ))}
            {(["half", "double"] as const).map((factor) => (
              <DropdownMenuItem
                key={factor}
                disabled={
                  !session.editor.context ||
                  modulationScaleUpdates(notesForPreset(), kind, factor)
                    .length === 0
                }
                onClick={() => {
                  const context = session.editor.context
                  if (!context) return
                  const updates = modulationScaleUpdates(
                    notesForPreset(),
                    kind,
                    factor
                  )
                  if (updates.length === 0) return
                  void dispatch({
                    type: "updateNotes",
                    pattern: context.pattern.id,
                    channel: context.channel,
                    updates: updates.map(({ id, expression }) => ({
                      id,
                      patch: { expression },
                    })),
                  })
                }}
              >
                {factor === "half" ? t("Half") : t("Double")}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        )}
        <DropdownMenuItem disabled={session.editor.notes.length === 0} onClick={openNoteLfo}>{t("Write LFO…")}</DropdownMenuItem>
        <DropdownMenuItem
          disabled={
            nextLaneHeightScale(usePianoRollStore.getState().laneHeight, "half") === null
          }
          onClick={() => {
            const next = nextLaneHeightScale(usePianoRollStore.getState().laneHeight, "half")
            if (next !== null) usePianoRollStore.getState().setLaneHeight(next)
          }}
        >
          {t("Halve lane height")}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={
            nextLaneHeightScale(usePianoRollStore.getState().laneHeight, "double") === null
          }
          onClick={() => {
            const next = nextLaneHeightScale(usePianoRollStore.getState().laneHeight, "double")
            if (next !== null) usePianoRollStore.getState().setLaneHeight(next)
          }}
        >
          {t("Double lane height")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** The bar between the grid and the lane. Drag it to give the lane more room. */
export function LaneResizer() {
  const height = usePianoRollStore((state) => state.laneHeight)
  const setHeight = usePianoRollStore((state) => state.setLaneHeight)
  const drag = useRef<{ y: number; height: number } | null>(null)
  const hint = useHint(t("Drag to resize the lane under the notes"))

  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label={t("Resize the lane under the notes")}
      aria-valuemin={MIN_LANE_HEIGHT}
      aria-valuemax={MAX_LANE_HEIGHT}
      aria-valuenow={height}
      tabIndex={0}
      className="h-[5px] cursor-ns-resize touch-none border-y bg-chassis outline-none hover:bg-brand/50 focus-visible:bg-brand/60 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset active:bg-brand/70"
      onPointerDown={(event) => {
        if (event.button !== 0) return
        drag.current = { y: event.clientY, height }
        event.currentTarget.setPointerCapture(event.pointerId)
      }}
      onPointerMove={(event) => {
        const start = drag.current
        if (start)
          setHeight(start.height + logicalDelta(start.y - event.clientY))
      }}
      onPointerUp={() => {
        drag.current = null
      }}
      onPointerCancel={() => {
        drag.current = null
      }}
      onKeyDown={(event) => {
        if (event.key === "ArrowUp") setHeight(height + KEY_STEP_PX)
        else if (event.key === "ArrowDown") setHeight(height - KEY_STEP_PX)
        else return
        event.preventDefault()
      }}
      {...hint}
    />
  )
}
