import type {
  AudioClipPatch,
  AudioClipUpdate,
  ClipId,
  Command,
  LibraryFileToken,
  SampleAsset,
  SampleId,
  TrackId,
} from "@/bindings"
import { mixerTrackOfSample, sampleOfFile } from "@/lib/audio-clips"
import { attempt, refuse } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { newGestureId } from "@/lib/store/gesture"
import { dispatch, receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { MASTER_TRACK, MAX_MIXER_TRACKS } from "@/lib/units"

import { clipInits, spanFits, tracksNeeded, type NewClip } from "../edit"
import { playlist, project, selectedClips } from "../selectors"
import { usePlaylistStore } from "../store"
import { wholeClipTicks } from "./geometry"
import { loadSamplePeaks } from "./peaks"
import { translatedText as t, message } from "@/lib/i18n"

/*
 * What the playlist does to audio clips: putting files and samples on the
 * timeline, and changing what only audio clips have. Each function is one
 * undo step.
 */

const ui = () => usePlaylistStore.getState()

/** The selected clips that are audio clips, in timeline order. */
export function selectedAudioClips() {
  return selectedClips().filter((clip) => clip.content.type === "audio")
}

/**
 * The mixer track a new clip of a sample plays into: the one the most
 * recent clip of that sample uses, so twenty clips of one loop share a
 * track. Undefined when there is no such clip, which makes a track for it.
 */
export function mixerTrackForSample(sample: SampleId): TrackId | undefined {
  return mixerTrackOfSample(project(), sample)
}

/**
 * The same for a file that is about to be put on the playlist: the track
 * of the clips that already play it. Undefined for a file the project does
 * not have yet, or has no clip of.
 */
export async function mixerTrackForFile(
  path: string
): Promise<TrackId | undefined> {
  if (playlist().clips.every((clip) => clip.content.type !== "audio")) {
    return undefined
  }
  // Which folders are the factory's is the shell's to say.
  const roots = await backend.browserRoots().catch(() => [])
  const sample = sampleOfFile(project().samples, path, {
    roots,
    projectPath: useProjectStore.getState().path,
  })
  return sample ? mixerTrackForSample(sample.id) : undefined
}

/** Changes what only audio clips have, for several clips in one command. */
export async function changeAudioClips(
  updates: readonly AudioClipUpdate[],
  gesture?: number
): Promise<boolean> {
  if (updates.length === 0) return true
  const done = await dispatch(
    { type: "updateAudioClips", updates: [...updates] },
    gesture
  )
  return done !== null
}

/** Gives every selected audio clip the same new settings. */
export function patchSelectedAudioClips(
  patch: AudioClipPatch,
  gesture?: number
): Promise<boolean> {
  return changeAudioClips(
    selectedAudioClips().map((clip) => ({ id: clip.id, patch })),
    gesture
  )
}

/**
 * Puts an audio file on the timeline as a clip that starts at `start`, on
 * the track of `row` or, where that row has no track yet, on a new track
 * at the end. It plays into the mixer track the clips of the same file
 * already use, and into a new one when it is the first: dropping a file
 * three times makes one mixer track, as placing it with the brush does.
 * Selects the clip. Resolves to its id.
 */
export async function addAudioFile(
  path: string,
  place: { row?: number; start: number },
  browser?: LibraryFileToken,
  current: () => boolean = () => true
): Promise<ClipId | null> {
  const generation = getProjectGeneration()
  const valid = () => generation === getProjectGeneration() && current()
  if (!valid()) return null
  const mixerTrack = await mixerTrackForFile(path)
  if (!valid()) return null
  const track =
    place.row === undefined ? undefined : playlist().tracks[place.row]?.id
  const target = {
    track,
    start: Math.max(0, Math.round(place.start)),
    mixerTrack,
  }
  const result = await attempt(
    browser
      ? backend.addAudioClipFromFile(path, target, browser)
      : backend.addAudioClipFromFile(path, target),
    t("Could not add the sound to the playlist")
  )
  if (!result || !valid()) return null
  // The same patch also arrives as an event; the store ignores the repeat.
  receivePatch(result.patch)
  const clip = result.created.at(-1)
  if (clip === undefined) return null
  ui().select([clip])
  return clip
}

/**
 * Routes the selected audio clips to a new mixer track named after the
 * first one's sample, as one undo step.
 */
export async function routeSelectionToNewTrack(): Promise<void> {
  const clips = selectedAudioClips()
  const [first] = clips
  if (!first || first.content.type !== "audio") return
  const sample = first.content.sample
  const name = project().samples.find((item) => item.id === sample)?.name
  const gesture = newGestureId()
  const added = await dispatch({ type: "addMixerTrack", name }, gesture)
  if (!added) return
  const mixerTrack = added.created[0]
  await dispatch(
    {
      type: "updateAudioClips",
      updates: clips.map((clip) => ({ id: clip.id, patch: { mixerTrack, output: "mixer" } })),
    },
    gesture
  )
}

/**
 * Places clips of a sample that is in the project's pool: what the Draw
 * and Paint tools do with an audio brush. One clip goes through the shell,
 * which reads its length from the audio. Several are laid down together,
 * with the tracks they need, as one undo step. Resolves to the new clips.
 */
export async function placeSampleClips(
  sample: SampleAsset,
  clips: readonly NewClip[],
  label: string
): Promise<ClipId[] | null> {
  if (clips.length === 0) return []
  if (!clips.every((clip) => spanFits(clip.start, clip.length))) {
    refuse(
      t("Clips cannot go past the end of the timeline"),
      t("The song is as long as it can get.")
    )
    return null
  }
  const reuse = mixerTrackForSample(sample.id)
  if (clips.length === 1) {
    const [clip] = clips
    const result = await attempt(
      backend.addAudioClipFromSample(sample.id, {
        track: playlist().tracks[clip.row]?.id,
        start: Math.max(0, Math.round(clip.start)),
        mixerTrack: reuse,
      }),
      t("Could not add the audio clip")
    )
    if (!result) return null
    receivePatch(result.patch)
    return result.created.slice(-1)
  }

  const peaks = await loadSamplePeaks(sample)
  if (peaks.status !== "ready") {
    refuse(
      message("\"{sampleName}\" cannot be placed", { sampleName: sample.name }),
      t("Its audio is not loaded. Check that its file is there, then reload the samples.")
    )
    return null
  }
  const length = wholeClipTicks(peaks.durationSecs, project().settings.tempoBpm)
  const gesture = newGestureId()
  const batch = (...commands: Command[]): Command => ({
    type: "batch",
    label,
    commands,
  })
  const missing = tracksNeeded(
    playlist().tracks.length,
    clips.map((clip) => clip.row)
  )
  if (missing > 0) {
    const added = await dispatch(
      batch(
        ...Array.from({ length: missing }, (): Command => ({
          type: "addPlaylistTrack",
        }))
      ),
      gesture
    )
    if (!added) return null
  }
  let mixerTrack = reuse
  if (mixerTrack === undefined) {
    if (project().mixer.tracks.length >= MAX_MIXER_TRACKS) {
      // A full mixer must not stop the user adding audio.
      mixerTrack = MASTER_TRACK
    } else {
      const added = await dispatch(
        batch({ type: "addMixerTrack", name: sample.name }),
        gesture
      )
      if (!added) return null
      mixerTrack = added.created[0]
    }
  }
  const tracks = playlist().tracks.map((track) => track.id)
  const placed = clips.map((clip): NewClip => ({
    ...clip,
    length,
    content:
      clip.content.type === "audio"
        ? { ...clip.content, mixerTrack }
        : clip.content,
  }))
  const added = await dispatch(
    batch({ type: "addClips", clips: clipInits(placed, tracks) }),
    gesture
  )
  return added?.created ?? null
}
