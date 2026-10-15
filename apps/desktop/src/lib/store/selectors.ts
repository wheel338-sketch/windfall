import { useShallow } from "zustand/react/shallow"

import type {
  Channel,
  ChannelId,
  HistoryView,
  Lane,
  MixerTrack,
  Pattern,
  PatternId,
  Project,
  ProjectSettings,
  SampleAsset,
  SampleId,
  TrackId,
} from "@/bindings"

import { useProjectStore } from "./project"
import { useTransportStore } from "./transport"
import { translatedText as t } from "@/lib/i18n"

/*
 * Each hook selects the smallest slice a component needs. Patches keep the
 * reference of everything that did not change, so `useChannel(3)` renders
 * again only when channel 3 does, and the id lists only when the order does.
 */

export function useProjectReady(): boolean {
  return useProjectStore((state) => state.ready)
}

export function useSettings(): ProjectSettings {
  return useProjectStore((state) => state.project.settings)
}

export function useTempo(): number {
  return useProjectStore((state) => state.project.settings.tempoBpm)
}

export function useChannelIds(): ChannelId[] {
  return useProjectStore(
    useShallow((state) => state.project.channels.map((channel) => channel.id))
  )
}

export function useChannel(id: ChannelId | null): Channel | undefined {
  return useProjectStore((state) =>
    state.project.channels.find((channel) => channel.id === id)
  )
}

export function useChannelCount(): number {
  return useProjectStore((state) => state.project.channels.length)
}

export function useMixerTrackIds(): TrackId[] {
  return useProjectStore(
    useShallow((state) => state.project.mixer.tracks.map((track) => track.id))
  )
}

export function useMixerTrack(id: TrackId | null): MixerTrack | undefined {
  return useProjectStore((state) =>
    state.project.mixer.tracks.find((track) => track.id === id)
  )
}

/** Position of a track in the mixer, which is also its place in the meters. */
export function useMixerTrackIndex(id: TrackId | null): number {
  return useProjectStore((state) =>
    state.project.mixer.tracks.findIndex((track) => track.id === id)
  )
}

/**
 * The number a mixer track shows on its strip: its place among the tracks,
 * the Current utility copy aside, with the master as 0. -1 when the track is
 * missing or is the Current copy.
 */
export function useMixerTrackNumber(id: TrackId | null): number {
  return useProjectStore((state) =>
    state.project.mixer.tracks.filter((track) => !track.current).findIndex((track) => track.id === id)
  )
}

export function usePatternIds(): PatternId[] {
  return useProjectStore(
    useShallow((state) => state.project.patterns.map((pattern) => pattern.id))
  )
}

export function usePattern(id: PatternId | null): Pattern | undefined {
  return useProjectStore((state) =>
    state.project.patterns.find((pattern) => pattern.id === id)
  )
}

/** The notes one channel plays in one pattern, or undefined when it has none. */
export function useLane(
  pattern: PatternId | null,
  channel: ChannelId | null
): Lane | undefined {
  return useProjectStore((state) =>
    state.project.patterns
      .find((item) => item.id === pattern)
      ?.lanes.find((lane) => lane.channel === channel)
  )
}

export function useSample(id: SampleId | null): SampleAsset | undefined {
  return useProjectStore((state) =>
    state.project.samples.find((sample) => sample.id === id)
  )
}

export function useHistory(): HistoryView {
  return useProjectStore((state) => state.history)
}

export function useCanUndo(): boolean {
  return useProjectStore((state) => state.history.cursor > 0)
}

export function useCanRedo(): boolean {
  return useProjectStore(
    (state) => state.history.cursor < state.history.entries.length
  )
}

export function useDirty(): boolean {
  return useProjectStore((state) => state.dirty)
}

export function useProjectPath(): string | null {
  return useProjectStore((state) => state.path)
}

/** A file's name without its folder and extension. */
function fileStem(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path
  return name.replace(/\.[^.]+$/, "") || name
}

/**
 * What the project is called wherever it is shown: the name of its file
 * once it has one, and until then the name in its settings.
 */
export function projectDisplayName(state: {
  path: string | null
  project: Project
}): string {
  if (state.path !== null) return fileStem(state.path)
  return state.project.settings.name || t("Untitled")
}

export function useProjectName(): string {
  return useProjectStore(projectDisplayName)
}

/** The transport's pattern, or the first pattern when that one is gone. */
export function selectedPatternId(
  project: Project,
  transportPattern: PatternId
): PatternId | null {
  if (project.patterns.some((pattern) => pattern.id === transportPattern)) {
    return transportPattern
  }
  return project.patterns[0]?.id ?? null
}

/**
 * The pattern being edited. It is always the pattern the transport plays in
 * pattern mode; change it with `setTransportPattern`.
 */
export function useSelectedPatternId(): PatternId | null {
  const transportPattern = useTransportStore((state) => state.pattern)
  return useProjectStore((state) =>
    selectedPatternId(state.project, transportPattern)
  )
}
