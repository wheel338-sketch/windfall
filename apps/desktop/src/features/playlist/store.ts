import { create } from "zustand"
import { persist } from "zustand/middleware"

import type {
  AutomationId,
  ClipId,
  PlaylistTrackId,
  SampleId,
} from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useSnapStore } from "@/lib/store/snap"

import { MAX_ROW_HEIGHT, MIN_ROW_HEIGHT } from "./layout"
import type { NewClip } from "./edit"
import type { Tool } from "./intents"
import type { SnapMode } from "./snap"

const NO_CLIPS: ReadonlySet<ClipId> = new Set()
const NO_HEIGHTS: ReadonlyMap<PlaylistTrackId, number> = new Map()
const NO_GROUPS: ReadonlySet<number> = new Set()
const resizingTracks = new Set<PlaylistTrackId>()

export function beginTrackResize(track: PlaylistTrackId) {
  resizingTracks.add(track)
}

export function endTrackResize(track: PlaylistTrackId) {
  resizingTracks.delete(track)
}

/**
 * What the Draw and Paint tools place. A pattern brush places the pattern
 * selected app-wide, so it carries no id of its own.
 */
export type Brush =
  | { type: "pattern" }
  | { type: "audio"; sample: SampleId }
  | { type: "automation"; automation: AutomationId }

const PATTERN_BRUSH: Brush = { type: "pattern" }

/** A point of a curve, as the clip it was pressed in shows it. */
export type PointRef = {
  clip: ClipId
  automation: AutomationId
  index: number
}

type PlaylistUiState = {
  tool: Tool
  snap: SnapMode
  /** Session-only: draw held steps across automation curve bodies. */
  step: boolean
  /** Keep the playhead in view while the song plays. */
  follow: boolean
  pickerOpen: boolean
  /** Show the settings of the selected audio clips above the timeline. */
  inspectorOpen: boolean
  /**
   * The right button deletes clips in every tool but Select, as in FL
   * Studio. Off, it opens the clip menu in every tool but Playback.
   */
  rightClickDeletes: boolean
  brush: Brush
  selection: ReadonlySet<ClipId>
  /** The track the track actions act on: the header last pressed. */
  targetTrack: PlaylistTrackId | null
  /** Session-only: opening a project starts with every group expanded. */
  collapsedGroups: ReadonlySet<number>
  /** Saved heights with live drag previews; missing tracks follow the global height. */
  trackHeights: ReadonlyMap<PlaylistTrackId, number>
  /**
   * Where the song will play from. In song mode it follows the playhead; in
   * pattern mode the engine keeps no song position, so the ruler sets it.
   */
  cursorTick: number
  /** Copied clips, with times counted from the earliest one. */
  clipboard: readonly NewClip[]
  /** What the grid's right-click menu was opened on. */
  menuOnClips: boolean
  /** The point of a curve the menu was opened on, if it was on one. */
  menuPoint: PointRef | null
  /** A clip the timeline should scroll to, once it is on screen. */
  reveal: ClipId | null
  /**
   * The timeline should take the keyboard, once it is on screen: what was
   * just selected in it is what the next key is meant for.
   */
  focusRequested: boolean

  setTool(tool: Tool): void
  setSnap(snap: SnapMode): void
  toggleStep(): void
  toggleFollow(): void
  togglePicker(): void
  toggleInspector(): void
  showInspector(): void
  toggleRightClickDeletes(): void
  setBrush(brush: Brush): void
  select(ids: Iterable<ClipId>): void
  clearSelection(): void
  setTargetTrack(track: PlaylistTrackId | null): void
  toggleGroupCollapse(group: number): void
  setTrackHeight(track: PlaylistTrackId, height: number): void
  setCursorTick(tick: number): void
  setClipboard(clips: readonly NewClip[]): void
  setMenuOnClips(onClips: boolean): void
  setMenuPoint(point: PointRef | null): void
  setReveal(clip: ClipId | null): void
  requestFocus(): void
  focusGiven(): void
}

/**
 * View state of the playlist that is not part of the project. The tool, the
 * layout choices survive a restart; snap belongs to the shared session store.
 */
export const usePlaylistStore = create<PlaylistUiState>()(
  persist(
    (set, get) => ({
      tool: "draw",
      snap: useSnapStore.getState().snap,
      step: false,
      follow: true,
      pickerOpen: true,
      inspectorOpen: true,
      rightClickDeletes: true,
      brush: PATTERN_BRUSH,
      selection: NO_CLIPS,
      targetTrack: null,
      collapsedGroups: NO_GROUPS,
      trackHeights: NO_HEIGHTS,
      cursorTick: 0,
      clipboard: [],
      menuOnClips: false,
      menuPoint: null,
      reveal: null,
      focusRequested: false,

      setTool: (tool) => set({ tool }),
      setSnap: (snap) => useSnapStore.getState().setSnap(snap),
      toggleStep: () => set((state) => ({ step: !state.step })),
      toggleFollow: () => set((state) => ({ follow: !state.follow })),
      togglePicker: () => set((state) => ({ pickerOpen: !state.pickerOpen })),
      toggleInspector: () =>
        set((state) => ({ inspectorOpen: !state.inspectorOpen })),
      showInspector: () => {
        if (!get().inspectorOpen) set({ inspectorOpen: true })
      },
      toggleRightClickDeletes: () =>
        set((state) => ({ rightClickDeletes: !state.rightClickDeletes })),
      setBrush: (brush) => {
        const current = get().brush
        const same =
          current.type === brush.type &&
          (brush.type !== "audio" ||
            (current.type === "audio" && current.sample === brush.sample)) &&
          (brush.type !== "automation" ||
            (current.type === "automation" &&
              current.automation === brush.automation))
        if (!same)
          set({ brush: brush.type === "pattern" ? PATTERN_BRUSH : brush })
      },
      select: (ids) => {
        const next = new Set(ids)
        const current = get().selection
        // Keeps the same set when nothing changed, so nothing redraws.
        if (
          next.size === current.size &&
          [...next].every((id) => current.has(id))
        ) {
          return
        }
        set({ selection: next.size === 0 ? NO_CLIPS : next })
      },
      clearSelection: () => {
        if (get().selection.size > 0) set({ selection: NO_CLIPS })
      },
      setTargetTrack: (targetTrack) => set({ targetTrack }),
      toggleGroupCollapse: (group) =>
        set((state) => {
          const collapsedGroups = new Set(state.collapsedGroups)
          if (collapsedGroups.has(group)) collapsedGroups.delete(group)
          else collapsedGroups.add(group)
          return { collapsedGroups }
        }),
      setTrackHeight: (track, height) => {
        if (!Number.isFinite(height)) return
        const next =
          height === 0
            ? 0
            : Math.min(
                MAX_ROW_HEIGHT,
                Math.max(MIN_ROW_HEIGHT, Math.round(height))
              )
        if ((get().trackHeights.get(track) ?? 0) === next) return
        const trackHeights = new Map(get().trackHeights)
        if (next === 0) trackHeights.delete(track)
        else trackHeights.set(track, next)
        set({ trackHeights })
      },
      setCursorTick: (tick) => {
        const cursorTick = Math.max(0, tick)
        if (cursorTick !== get().cursorTick) set({ cursorTick })
      },
      setClipboard: (clipboard) => set({ clipboard }),
      setMenuOnClips: (menuOnClips) => {
        if (menuOnClips !== get().menuOnClips) set({ menuOnClips })
      },
      setMenuPoint: (menuPoint) => {
        if (menuPoint !== get().menuPoint) set({ menuPoint })
      },
      setReveal: (reveal) => {
        if (reveal !== get().reveal) set({ reveal })
      },
      requestFocus: () => set({ focusRequested: true }),
      focusGiven: () => {
        if (get().focusRequested) set({ focusRequested: false })
      },
    }),
    {
      name: "windfall.playlist",
      version: 1,
      partialize: (state) => ({
        tool: state.tool,
        follow: state.follow,
        pickerOpen: state.pickerOpen,
        inspectorOpen: state.inspectorOpen,
        rightClickDeletes: state.rightClickDeletes,
      }),
      // Ignore snap in preferences saved before it became a session choice.
      merge: (saved, current) => ({
        ...current,
        ...(saved as Partial<PlaylistUiState>),
        snap: useSnapStore.getState().snap,
      }),
    }
  )
)

// Keep the existing editor read interface as a projection of the shared owner.
useSnapStore.subscribe(({ snap }) => usePlaylistStore.setState({ snap }))

/** Every document update restores the saved copy except under a held pointer. */
export function syncTrackHeights() {
  const tracks = useProjectStore.getState().project.playlist.tracks
  const current = usePlaylistStore.getState().trackHeights
  const trackHeights = new Map<PlaylistTrackId, number>()
  for (const track of tracks) {
    const height = resizingTracks.has(track.id)
      ? current.get(track.id)
      : track.height
    if (height) trackHeights.set(track.id, height)
  }
  if (
    current.size !== trackHeights.size ||
    [...trackHeights].some(([id, height]) => current.get(id) !== height)
  )
    usePlaylistStore.setState({ trackHeights })
}

syncTrackHeights()
useProjectStore.subscribe((state, previous) => {
  if (state.project.playlist.tracks !== previous.project.playlist.tracks)
    syncTrackHeights()
})

// Ids start over in every project, so whatever points into the old one is
// dropped: a clip with the same id in the new project is not the clip that
// was selected.
onProjectReplaced(() => {
  resizingTracks.clear()
  usePlaylistStore.setState({
    step: false,
    selection: NO_CLIPS,
    targetTrack: null,
    collapsedGroups: NO_GROUPS,
    trackHeights: NO_HEIGHTS,
    cursorTick: 0,
    clipboard: [],
    menuOnClips: false,
    menuPoint: null,
    reveal: null,
    brush: PATTERN_BRUSH,
  })
  syncTrackHeights()
})
