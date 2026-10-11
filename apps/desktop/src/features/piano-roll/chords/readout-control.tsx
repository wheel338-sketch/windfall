import {
  useCallback,
  useEffect,
  useId,
  useState,
  useSyncExternalStore,
} from "react"

import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"

import { useSession } from "../context"
import { PITCH_NAMES } from "./detection"
import { heldKeys, releaseHeldKeys, subscribeHeldKeys } from "./held-keys"
import {
  CHORD_SOURCE_LABELS,
  createChordReadoutReader,
  NO_CHORD,
  type ChordReadoutView,
} from "./readout"
import { useChordDisplayStore } from "./readout-panel"

const names = (pitchClasses: readonly number[]) =>
  pitchClasses.map((pc) => PITCH_NAMES[pc]).join(" · ")

function ChordFacts({ view }: { view: ChordReadoutView }) {
  const chord = view.detection
  if (!chord || chord.kind === "empty") {
    return (
      <p className="text-muted-foreground">
        Hold keys, select notes or move the playhead over notes to name their
        chord.
      </p>
    )
  }
  const root = chord.root ?? chord.bass
  const facts: [string, string][] = [
    ["Source", CHORD_SOURCE_LABELS[view.source]],
    ["Root", chord.root === null ? NO_CHORD : PITCH_NAMES[chord.root]],
    ["Bass", chord.bass === null ? NO_CHORD : PITCH_NAMES[chord.bass]],
    ["Pitch classes", names(chord.pitchClasses)],
    [
      chord.root === null ? "Intervals above bass" : "Intervals",
      chord.degrees.join(" · "),
    ],
  ]
  if (chord.omitted.length && root !== null) {
    facts.push(["Left out", names(chord.omitted.map((i) => (root + i) % 12))])
  }
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
      {facts.map(([term, value]) => (
        <div key={term} className="contents">
          <dt className="text-muted-foreground">{term}</dt>
          <dd className="font-readout text-foreground">{value}</dd>
        </div>
      ))}
    </dl>
  )
}

/**
 * The chord being played, selected or under the playhead: a compact readout
 * in the toolbar that opens a pinned display with the details. It follows
 * the editor, the playhead and the held keys without React state, and
 * renders again only when what it shows changes.
 */
export function ChordReadout() {
  const session = useSession()
  const { editor } = session
  const [read] = useState(createChordReadoutReader)
  const open = useChordDisplayStore((state) => state.open)
  const nameId = useId()
  // The pointer moving over the grid changes neither notes nor selection.
  const subscribeEditor = useCallback(
    (listener: () => void) =>
      editor.subscribe((event) => {
        if (event !== "hover") listener()
      }),
    [editor]
  )
  const subscribe = useCallback(
    (listener: () => void) => {
      const stops = [
        subscribeEditor(listener),
        session.onPlayhead(listener),
        subscribeHeldKeys(listener),
      ]
      return () => {
        for (const stop of stops) stop()
      }
    },
    [subscribeEditor, session]
  )
  const view = useSyncExternalStore(subscribe, () =>
    read(
      editor.notes,
      editor.selection,
      session.playhead,
      heldKeys(editor.context?.channel)
    )
  )
  const chord = view.detection
  const channel = useSyncExternalStore(
    subscribeEditor,
    () => editor.context?.channel ?? null
  )
  // A key whose release never arrives must not outlive the lane it was
  // played on, nor the readout itself.
  useEffect(() => {
    if (channel === null) return
    return () => releaseHeldKeys(channel)
  }, [channel])

  return (
    <Popover
      open={open}
      onOpenChange={(next, details) => {
        if (!next) {
          const target = details.event.target
          const inside =
            target instanceof Element &&
            target.closest("[data-chord-display]") !== null
          // Pinned: playing, drawing and selecting around it keep it up, and
          // Escape stays the editor's unless the display itself has focus.
          if (
            details.reason === "outside-press" ||
            details.reason === "focus-out" ||
            (details.reason === "escape-key" && !inside)
          ) {
            details.cancel()
            return
          }
        }
        useChordDisplayStore.getState().setOpen(next)
      }}
    >
      <PopoverTrigger
        // The name says what the button is; its text, the chord, describes it.
        aria-label="Chord"
        aria-describedby={nameId}
        data-chord-display=""
        title={chord ? `${chord.symbol} · ${chord.label}` : "Chord display"}
        className="max-w-56 shrink-0 truncate rounded-sm bg-display px-2 py-1 font-readout text-[0.6875rem] text-display-foreground outline-none hover:brightness-110 focus-visible:ring-2 focus-visible:ring-ring aria-expanded:ring-1 aria-expanded:ring-ring"
      >
        <span id={nameId}>{view.label}</span>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        data-chord-display=""
        // Focus stays in the grid, so typed and played keys keep sounding.
        initialFocus={false}
        finalFocus={false}
        className="w-64 gap-3"
      >
        <PopoverHeader>
          <PopoverTitle className="sr-only">Chord display</PopoverTitle>
          <p
            data-slot="chord-symbol"
            className="font-readout text-xl leading-tight text-foreground"
          >
            {chord?.symbol || NO_CHORD}
          </p>
          <PopoverDescription data-slot="chord-name">
            {chord ? chord.label : "No chord"}
          </PopoverDescription>
        </PopoverHeader>
        <ChordFacts view={view} />
      </PopoverContent>
    </Popover>
  )
}
