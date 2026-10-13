import type {
  Automation,
  AutomationTarget,
  ChannelId,
  EffectId,
  Project,
  TrackId,
} from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { message, plainNumber } from "@/lib/i18n"

/*
 * What goes with a thing when it is deleted. An automation lives as long
 * as what it moves: removing a channel, a mixer track, an effect or a send
 * removes the automations of it and their clips in the same undo step.
 * Every deletion that does so says so first, and this is where it finds
 * out what to say.
 */

/** A thing whose deletion takes automations with it. */
export type Owner =
  | { type: "channel"; channel: ChannelId }
  /** A mixer track, with its effects and the sends from and to it. */
  | { type: "track"; track: TrackId }
  | { type: "effect"; effect: EffectId }
  | { type: "send"; track: TrackId; target: TrackId }
  | { type: "sidechain"; track: TrackId; target: TrackId }

export type Owned = {
  /** The automations that are deleted with the owner. */
  automations: Automation[]
  /** How many clips on the playlist show them. */
  clips: number
}

function belongsTo(
  target: AutomationTarget,
  owner: Owner,
  effectsOfTrack: ReadonlySet<EffectId>
): boolean {
  switch (owner.type) {
    case "channel":
      return (
        (target.type === "channelVolume" ||
          target.type === "channelPan" ||
          target.type === "instrumentParam") &&
        target.channel === owner.channel
      )
    case "effect":
      return (
        (target.type === "effectParam" || target.type === "effectMix") &&
        target.effect === owner.effect
      )
    case "sidechain":
      return target.type === "sidechainGain" && target.track === owner.track && target.target === owner.target
    case "send":
      return (
        target.type === "sendGain" &&
        target.track === owner.track &&
        target.target === owner.target
      )
    case "track":
      switch (target.type) {
        case "trackVolume":
        case "trackPan":
        case "trackParam":
          return target.track === owner.track
        // A send goes when either end of it does.
        case "sendGain":
        case "sidechainGain":
          return target.track === owner.track || target.target === owner.track
        // An effect is the track's for as long as it sits in its chain,
        // wherever the automation was made.
        case "effectParam":
        case "effectMix":
          return effectsOfTrack.has(target.effect)
        default:
          return false
      }
    default: {
      const _exhaustive: never = owner
      return _exhaustive
    }
  }
}

/** The automations that are deleted with `owner`, and their clips. */
export function automationOwnedBy(
  project: Pick<Project, "automations" | "mixer" | "playlist">,
  owner: Owner
): Owned {
  const effects = new Set<EffectId>(
    owner.type === "track"
      ? (project.mixer.tracks
          .find((track) => track.id === owner.track)
          ?.effects.map((slot) => slot.id) ?? [])
      : []
  )
  const automations = project.automations.filter((automation) =>
    belongsTo(automation.target, owner, effects)
  )
  const ids = new Set(automations.map((automation) => automation.id))
  const clips = project.playlist.clips.filter(
    (clip) =>
      clip.content.type === "automation" && ids.has(clip.content.automation)
  ).length
  return { automations, clips }
}

/**
 * One sentence on what `owned` holds, for a question about deleting its
 * owner, or null when it holds nothing and there is nothing to say.
 */
export function ownedSentence({ automations, clips }: Owned): string | null {
  const count = automations.length
  if (count === 0) return null
  if (count === 1) {
    const name = message("The automation \"{automationsName}\"", { automationsName: automations[0].name })
    if (clips === 0) return message("{name} is deleted with it.", { name })
    return clips === 1
      ? message("{name} and its clip on the playlist are deleted with it.", { name })
      : message("{name} and its {clips} clips on the playlist are deleted with it.", { name, clips: plainNumber(clips) })
  }
  if (clips === 0) return message("{count} automations are deleted with it.", { count: plainNumber(count) })
  return clips === 1
    ? message("{count} automations and their clip on the playlist are deleted with it.", { count: plainNumber(count) })
    : message("{count} automations and their {clips} clips on the playlist are deleted with it.", { count: plainNumber(count), clips: plainNumber(clips) })
}

/** What deleting `owner` from the open project takes with it, as a sentence. */
export function automationGoingWith(owner: Owner): string | null {
  return ownedSentence(
    automationOwnedBy(useProjectStore.getState().project, owner)
  )
}
