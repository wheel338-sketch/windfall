import type {
  Clip,
  Command,
  DispatchResult,
  ExportOptions,
  ExportPlaylistStems,
  MidiHardwareNote,
  PlaylistTrackId,
  Project,
  SampleInfo,
} from "@/bindings"
import { MASTER_TRACK } from "@/lib/units"

import type { Backend, Unsubscribe } from "../backend"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export type PlaylistMockOptions = {
  /**
   * Test render fixture for track consolidation: the audio file that stands
   * in for one lane's print. Browser previews have no offline renderer and
   * refuse consolidation without it.
   */
  renderPlaylistTrack?: (
    source: Project,
    track: PlaylistTrackId,
    region: { start: number; end: number }
  ) => Promise<string>
}

/** What the mock's own helpers give this module, as the shell has them. */
export type PlaylistMockHost = {
  /** The live project and the revision it is at. */
  snapshot(): { project: Project; revision: number }
  ipc<T>(work: () => T): Promise<T>
  dispatch(command: Command): DispatchResult
  /** The id a file's sample has or will get, and the command that adds it. */
  sampleCommands(path: string): {
    sampleId: number
    addSample: Command
  }
  audioClipCommands(
    sample: number,
    durationSecs: number,
    name: string,
    place: { track: PlaylistTrackId; start: number; mixerTrack: number },
    nextId: number
  ): Command[]
  sampleInfo(path: string): SampleInfo
  exportAudio(options: ExportOptions, request?: string): Promise<void>
}

export type PlaylistMock = Pick<
  Backend,
  "consolidatePlaylistTracks" | "exportPlaylistTracks" | "onMidiHardwareNote"
> & {
  /** Stands in for a note arriving from a MIDI input. */
  playMidiHardwareNote(note: MidiHardwareNote): void
}

/** Whether the active arrangement shows a clip, as the shell decides it. */
export function layoutShows(project: Project, clip: Clip): boolean {
  const book = project.playlist.arrangementBook
  if (!book || book.active == null) return true
  const active = book.arrangements.find((item) => item.id === book.active)
  if (!active) return true
  return active.clips.includes(clip.id) && active.tracks.includes(clip.track)
}

/** The lanes the active arrangement shows, in its order. */
export function layoutTracks(project: Project): Project["playlist"]["tracks"] {
  const { tracks, arrangementBook: book } = project.playlist
  if (!book || book.active == null || book.arrangements.length === 0)
    return tracks
  const active = book.arrangements.find((item) => item.id === book.active)
  if (!active) return tracks
  return active.tracks.flatMap((id) => {
    const track = tracks.find((item) => item.id === id)
    return track ? [track] : []
  })
}

const sounds = (clip: Clip) => clip.content.type !== "automation"

export type ConsolidateRecipe = {
  track: PlaylistTrackId
  name: string
  sources: number[]
  start: number
  end: number
}

/**
 * What consolidating these lanes renders and mutes, in playlist order:
 * each lane's unmuted pattern and audio clips that the active arrangement
 * shows. Throws the shell's message when a lane cannot be consolidated.
 */
export function consolidatePlan(
  project: Project,
  tracks: readonly PlaylistTrackId[]
): ConsolidateRecipe[] {
  if (tracks.length === 0)
    throw new Error(t("Choose at least one playlist track to consolidate."))
  const lanes = project.playlist.tracks
  for (const id of tracks) {
    if (!lanes.some((track) => track.id === id))
      throw new Error(t("A chosen playlist track no longer exists."))
  }
  const wanted = new Set(tracks)
  return lanes
    .filter((track) => wanted.has(track.id))
    .map((track) => {
      const sources = project.playlist.clips.filter(
        (clip) =>
          clip.track === track.id &&
          !clip.muted &&
          clip.length > 0 &&
          sounds(clip) &&
          layoutShows(project, clip)
      )
      if (sources.length === 0)
        throw new Error(
          message("\"{trackName}\" has no unmuted pattern or audio clip to consolidate.", { trackName: track.name })
        )
      return {
        track: track.id,
        name: `${track.name} (consolidated)`,
        sources: sources.map((clip) => clip.id),
        start: Math.min(...sources.map((clip) => clip.start)),
        end: Math.max(...sources.map((clip) => clip.start + clip.length)),
      }
    })
}

/** The copy a recipe is rendered from: the lane's sources alone, audible. */
export function consolidateSource(
  project: Project,
  recipe: ConsolidateRecipe
): Project {
  const chosen = new Set(recipe.sources)
  const source = structuredClone(project)
  for (const clip of source.playlist.clips) {
    if (!chosen.has(clip.id) && sounds(clip)) clip.muted = true
  }
  for (const track of source.playlist.tracks) {
    track.solo = false
    if (track.id === recipe.track) track.muted = false
  }
  return source
}

/**
 * The lanes an export of the playlist tracks writes: the ones the mix plays
 * that hold an unmuted pattern or audio clip, in playlist order. Throws the
 * shell's message for a request it would refuse.
 */
export function playlistStemTracks(
  project: Project,
  stems: ExportPlaylistStems
): PlaylistTrackId[] {
  const lanes = layoutTracks(project)
  const anySolo = lanes.some((track) => track.solo)
  const sounding = new Set(
    project.playlist.clips
      .filter(
        (clip) =>
          !clip.muted &&
          clip.length > 0 &&
          sounds(clip) &&
          layoutShows(project, clip)
      )
      .map((clip) => clip.track)
  )
  const giving = lanes
    .filter(
      (track) =>
        !track.muted && (!anySolo || track.solo) && sounding.has(track.id)
    )
    .map((track) => track.id)
  if (stems.tracks === undefined) {
    if (giving.length === 0)
      throw new Error(
        t("No playlist track plays a pattern or audio clip, so there are no tracks to export.")
      )
    return giving
  }
  for (const id of stems.tracks) {
    if (!lanes.some((track) => track.id === id))
      throw new Error(
        message("The playlist has no track with the id {id} in this arrangement.", { id: plainNumber(id) })
      )
    if (!giving.includes(id))
      throw new Error(
        message("Playlist track {id} plays nothing: it is muted, silenced by solo, or holds no pattern or audio clip.", { id: plainNumber(id) })
      )
  }
  const chosen = giving.filter((id) => stems.tracks!.includes(id))
  if (chosen.length === 0 && !stems.includeMix)
    throw new Error(t("Choose at least one playlist track to export."))
  return chosen
}

export function createPlaylistMock(
  options: PlaylistMockOptions,
  host: PlaylistMockHost
): PlaylistMock {
  const notes = new Set<(note: MidiHardwareNote) => void>()
  return {
    async consolidatePlaylistTracks(tracks) {
      const captured = await host.ipc(() => host.snapshot())
      const recipes = consolidatePlan(captured.project, tracks)
      const render = options.renderPlaylistTrack
      if (!render)
        throw new Error(
          t("Consolidating requires the desktop app's offline renderer. No project changes were made.")
        )
      const files: string[] = []
      for (const recipe of recipes) {
        files.push(
          await render(
            consolidateSource(captured.project, recipe),
            recipe.track,
            { start: recipe.start, end: recipe.end }
          )
        )
      }
      return host.ipc(() => {
        const current = host.snapshot()
        if (current.revision !== captured.revision)
          throw new Error(
            t("The project or its audio changed while consolidating. Choose the tracks and try again.")
          )
        // A batch cannot pass an id on, so the ids are worked out here in
        // the order the commands allocate them: a sample, then its clip.
        let next = current.project.nextId
        const planned = new Map<string, number>()
        const commands: Command[] = []
        recipes.forEach((recipe, index) => {
          const file = files[index]
          const sample = host.sampleCommands(file)
          let sampleId = sample.sampleId
          // An id at the project's cursor means the file is new to the pool.
          if (sampleId === current.project.nextId) {
            const earlier = planned.get(file)
            if (earlier === undefined) {
              sampleId = next
              next += 1
              planned.set(file, sampleId)
            } else {
              sampleId = earlier
            }
          }
          commands.push({ ...sample.addSample, name: recipe.name } as Command)
          const clipCommands = host.audioClipCommands(
            sampleId,
            host.sampleInfo(file).durationSecs,
            recipe.name,
            { track: recipe.track, start: recipe.start, mixerTrack: MASTER_TRACK },
            next
          )
          next += 1
          for (const command of clipCommands) {
            if (command.type !== "addClips") continue
            for (const clip of command.clips) {
              clip.length = recipe.end - recipe.start
              clip.muted = false
              // The render already passed the whole mixer.
              if (clip.content.type === "audio") clip.content.output = "direct"
            }
          }
          commands.push(...clipCommands)
        })
        commands.push({
          type: "updateClips",
          updates: recipes
            .flatMap((recipe) => recipe.sources)
            .map((id) => ({ id, patch: { muted: true } })),
        })
        return host.dispatch({
          type: "batch",
          label: recipes.length === 1 ? t("Consolidate track") : t("Consolidate tracks"),
          commands,
        })
      })
    },

    async exportPlaylistTracks(exportOptions, stems, request) {
      await host.ipc(() => {
        if (exportOptions.stems)
          throw new Error(
            t("Export the mixer tracks or the playlist tracks apart, not both at once.")
          )
        if (exportOptions.mode !== "song")
          throw new Error(
            t("Playlist tracks belong to the song. Export the song to get a file for each of them.")
          )
        playlistStemTracks(host.snapshot().project, stems)
      })
      // The browser writes no files; the progress is the plain export's.
      return host.exportAudio(exportOptions, request)
    },

    onMidiHardwareNote(handler): Unsubscribe {
      notes.add(handler)
      return () => {
        notes.delete(handler)
      }
    },

    playMidiHardwareNote(note) {
      for (const handler of [...notes]) handler(note)
    },
  }
}
