import { ArrowRight02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import descriptors from "@/bindings/descriptors.json"
import type {
  EffectId,
  EffectOutputRoute,
  EffectSlot,
  MixerTrack,
  TrackId,
} from "@/bindings"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { dispatch, useHint, useProjectStore } from "@/lib/store"
import { MASTER_TRACK } from "@/lib/units"
import { cn } from "@/lib/utils"

import { effectName } from "./effect-ops"
import { LoopNote, TrackLabel } from "./output-select"
import { outputChoices, type RoutingChoices } from "./routing"

/*
 * Routing of the outputs some effects have beside the next place in their
 * chain: Send Tap's copy of the signal, and the transient and sustained
 * parts of Transient Split and Transient Shaper. The engine sends a routed
 * output to its track from the effect's place in the chain, before the fader.
 */

/** One separately routable output of an effect kind. */
export type EffectOutputInfo = {
  id: string
  label: string
  /** Routing it takes it out of the chain instead of copying it. */
  splits: boolean
}

const NO_OUTPUTS: readonly EffectOutputInfo[] = []
const NO_ROUTES: readonly EffectOutputRoute[] = []
const NONE = "none"

// The table comes from the engine with the parameter tables, so the two
// never disagree about which effects have outputs or what they are called.
const OUTPUTS: Partial<Record<string, readonly EffectOutputInfo[]>> =
  (
    descriptors as unknown as {
      effectOutputs?: Record<string, readonly EffectOutputInfo[]>
    }
  ).effectOutputs ?? {}

/** The routable outputs of an effect kind, in the order routes number them. */
export function effectOutputsOf(kind: string): readonly EffectOutputInfo[] {
  return OUTPUTS[kind] ?? NO_OUTPUTS
}

type Tracks = readonly MixerTrack[]

/**
 * The tracks with every effect-output route added as an edge of the track
 * its effect sits on, so the mixer's loop test sees them. The extra edges
 * are stand-ins: only where they lead matters.
 */
export function withEffectOutputEdges(
  tracks: Tracks,
  routes: readonly EffectOutputRoute[]
): Tracks {
  if (routes.length === 0) return tracks
  return tracks.map((track) => {
    const extra = routes
      .filter(
        (route) =>
          route.target !== track.id &&
          track.effects.some((slot) => slot.id === route.effect)
      )
      .map((route) => ({ target: route.target, gain: 1 }))
    return extra.length ? { ...track, sends: [...track.sends, ...extra] } : track
  })
}

/**
 * Where output `output` of `effect`, on track `track`, may be sent: every
 * other track that does not already lead back to this one. The route being
 * replaced is left out of the loop test, as the backend leaves it out.
 */
export function effectOutputChoices(
  tracks: Tracks,
  routes: readonly EffectOutputRoute[],
  track: TrackId,
  effect: EffectId,
  output: number
): RoutingChoices {
  const others = routes.filter(
    (route) => !(route.effect === effect && route.output === output)
  )
  return outputChoices(withEffectOutputEdges(tracks, others), track)
}

/** The route of one output, if it has one. */
export function effectOutputRoute(
  routes: readonly EffectOutputRoute[] | undefined,
  effect: EffectId,
  output: number
): EffectOutputRoute | undefined {
  return routes?.find(
    (route) => route.effect === effect && route.output === output
  )
}

export function setEffectOutput(
  effect: EffectId,
  output: number,
  target: TrackId | null
) {
  return dispatch({
    type: "setEffectOutput",
    effect,
    output,
    ...(target === null ? {} : { target }),
  })
}

// Mounted only while the menu is open, so it can follow every track.
function Choices({
  track,
  effect,
  output,
  info,
  target,
}: {
  track: TrackId
  effect: EffectId
  output: number
  info: EffectOutputInfo
  target: TrackId | null
}) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const routes = useProjectStore(
    (state) => state.project.mixer.effectOutputs ?? NO_ROUTES
  )
  const { tracks: open, looping } = effectOutputChoices(
    tracks,
    routes,
    track,
    effect,
    output
  )
  return (
    <>
      <DropdownMenuRadioGroup
        value={target === null ? NONE : String(target)}
        onValueChange={(value: string) =>
          void setEffectOutput(
            effect,
            output,
            value === NONE ? null : Number(value)
          )
        }
      >
        {open.map((item) => (
          <DropdownMenuRadioItem
            key={item.id}
            value={String(item.id)}
            closeOnClick
          >
            {/* `item` may be a stand-in with extra edges, so find it by id. */}
            <TrackLabel
              track={item}
              number={tracks.findIndex((other) => other.id === item.id)}
            />
          </DropdownMenuRadioItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuRadioItem value={NONE} closeOnClick>
          {info.splits ? "Not routed (stays in the chain)" : "Not routed"}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
      <LoopNote count={looping} />
    </>
  )
}

function OutputRow({
  track,
  effect,
  name,
  output,
  info,
}: {
  track: TrackId
  effect: EffectId
  name: string
  output: number
  info: EffectOutputInfo
}) {
  const target = useProjectStore(
    (state) =>
      effectOutputRoute(state.project.mixer.effectOutputs, effect, output)
        ?.target ?? null
  )
  const targetName = useProjectStore((state) =>
    target === null
      ? null
      : (state.project.mixer.tracks.find((item) => item.id === target)?.name ??
        null)
  )
  const hint = useHint(
    info.splits
      ? `Where the ${info.label.toLowerCase()} part goes. Routed, it leaves this track's chain and plays on the chosen track, where it can have effects of its own`
      : "Where the copy goes: the signal as it is at this point in the chain, before this track's fader"
  )
  return (
    <li className="flex min-w-0 items-center gap-1.5">
      <span className="w-16 shrink-0 truncate text-[10px] text-muted-foreground">
        {info.label}
      </span>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <button
              type="button"
              data-slot="effect-output"
              aria-label={`${info.label} output of ${name}: ${targetName ?? "not routed"}`}
              className={cn(
                "flex h-[18px] min-w-0 flex-1 items-center gap-0.5 rounded-[3px] bg-muted/60 pr-1 pl-0.5 text-[10px] leading-none text-foreground/85 outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground",
                target === null && "text-muted-foreground italic"
              )}
              {...hint}
            />
          }
        >
          <HugeiconsIcon
            icon={ArrowRight02Icon}
            strokeWidth={2}
            className="size-3 shrink-0 text-muted-foreground"
          />
          <span className="truncate">{targetName ?? "Not routed"}</span>
        </DropdownMenuTrigger>
        <DropdownMenuContent className="max-h-80 w-auto min-w-44">
          <Choices
            track={track}
            effect={effect}
            output={output}
            info={info}
            target={target}
          />
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  )
}

/**
 * The output routing of one effect, under its editor. Nothing for an effect
 * that has no outputs of its own.
 */
export function EffectOutputs({
  track,
  slot,
}: {
  track: TrackId
  slot: EffectSlot
}) {
  const outputs = effectOutputsOf(slot.params.type)
  const current = useProjectStore(
    (state) =>
      state.project.mixer.tracks.find((item) => item.id === track)?.current ===
      true
  )
  if (outputs.length === 0) return null
  const name = effectName(slot.params.type)
  // The master is the end of the mix and Current only listens, so neither
  // can feed another track.
  if (track === MASTER_TRACK || current) {
    return (
      <p
        data-slot="effect-outputs"
        className="mt-1.5 border-t pt-1.5 text-[10px] leading-snug text-muted-foreground"
      >
        {track === MASTER_TRACK
          ? `On the master, ${name} has nowhere to send: every track already plays into it. Put it on an insert track to route its ${outputs.length === 1 ? "output" : "outputs"}.`
          : `${name} cannot send from the Current track.`}
      </p>
    )
  }
  return (
    <ul
      data-slot="effect-outputs"
      aria-label={`${name} output routing`}
      className="mt-1.5 space-y-1 border-t pt-1.5"
    >
      {outputs.map((info, output) => (
        <OutputRow
          key={info.id}
          track={track}
          effect={slot.id}
          name={name}
          output={output}
          info={info}
        />
      ))}
    </ul>
  )
}
