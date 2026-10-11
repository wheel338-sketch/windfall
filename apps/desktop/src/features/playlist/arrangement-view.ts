import type { Playlist } from "@/bindings"

/** Resolve references for the view without changing the saved playlist. */
export function arrangementPlaylist(playlist: Playlist): Playlist {
  const book = playlist.arrangementBook
  if (!book?.arrangements.length || book.active == null) return playlist
  const active = book.arrangements.find((item) => item.id === book.active)
  const byId = new Map(playlist.tracks.map((track) => [track.id, track]))
  // An arrangement with no clips yet still shows its lanes, so that the
  // first clip of a new layout has somewhere to go.
  const tracks = active
    ? active.tracks.flatMap((id) => {
        const track = byId.get(id)
        return track ? [track] : []
      })
    : []
  const trackIds = new Set(tracks.map((track) => track.id))
  const clipIds = new Set(active?.clips)
  return {
    ...playlist,
    tracks,
    clips: playlist.clips.filter(
      (clip) => clipIds.has(clip.id) && trackIds.has(clip.track)
    ),
  }
}
