import {
  invalidateActionsOn,
  registry,
  type Action,
  type AppState,
} from "@/lib/actions"
import { useUiStore } from "@/lib/store"
import { useProjectStore } from "@/lib/store/project"
import {
  openRecording,
  useRecordingStore,
} from "@/features/transport/recording-store"
import { MASTER_TRACK, MAX_MIXER_TRACKS } from "@/lib/units"
import { showCurrentUtility } from "./current-source"
import { selectedMixerTracks, useMixerUi, visualMixerOrder } from "./mixer-ui"
import { mutedTrackIds, soloTrackIds } from "./select-flags"
import { routedTrackIds } from "./select-routed"
import {
  openMixerRender,
  renderableMixerTracks,
} from "@/features/export/mixer-render"

import { EFFECT_ACTIONS } from "./effect-actions"
import { effectEnableUpdates } from "./effect-enable"
import {
  bypassSelectedEffects,
  enableSelectedEffects,
  keepEffectOnSelectedTrack,
} from "./effect-ops"
import { useEffectsUi } from "./effects-ui"
import {
  addTrack,
  centerPan,
  deleteTrack,
  moveSelection,
  resetAllPeaks,
  resetLevels,
  resetVolume,
  setOutput,
  startColoring,
  startRename,
  toggleMute,
  toggleSolo,
  unmuteAll,
  unsoloAll,
  patchTrack,
} from "./operations"
import { dynamicTitle, translatedText as t } from "@/lib/i18n"

const SECTION = "Mixer"

function selected(state: AppState) {
  return state.document.project.mixer.tracks.find(
    (track) => track.id === state.ui.selectedTrack
  )
}

function selectedInsert(state: AppState) {
  const track = selected(state)
  return track && track.id !== MASTER_TRACK ? track : undefined
}

/** Runs `work` on the selected track. Does nothing when there is none. */
function onSelected(work: (id: number) => void | Promise<void>) {
  return () => {
    const id = useUiStore.getState().selectedTrack
    if (id !== null) return work(id)
  }
}

/**
 * What can be done to the mixer and to the selected track. The plain keys
 * belong to the mixer: they work while it has the keyboard.
 */
export const MIXER_ACTIONS: Action[] = [
  {
    id: "mixer.bypassEffects",
    ...dynamicTitle("Bypass effects", {}),
    section: SECTION,
    enabled: () => effectEnableUpdates(selectedMixerTracks(), false).length > 0,
    run: bypassSelectedEffects,
  },
  {
    id: "mixer.enableEffects",
    ...dynamicTitle("Enable effects", {}),
    section: SECTION,
    enabled: () => effectEnableUpdates(selectedMixerTracks(), true).length > 0,
    run: enableSelectedEffects,
  },
  {
    id: "mixer.extendNext",
    ...dynamicTitle("Extend mixer selection right", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "Shift+ArrowRight",
    repeats: true,
    run: () => moveSelection(1, true),
  },
  {
    id: "mixer.extendPrevious",
    ...dynamicTitle("Extend mixer selection left", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "Shift+ArrowLeft",
    repeats: true,
    run: () => moveSelection(-1, true),
  },
  {
    id: "mixer.currentUtility",
    ...dynamicTitle("Show Current mixer utility", {}),
    section: SECTION,
    keywords: "selected follow utility analysis meter effects",
    run: showCurrentUtility,
  },
  {
    id: "mixer.toggleRecordArm",
    ...dynamicTitle("Arm or disarm mixer track for recording", {}),
    section: SECTION,
    keywords: "record input microphone interface",
    enabled: (state) => {
      const recording = selected(state)?.recording
      return (
        !!recording &&
        (!!recording.input || (recording.mode ?? "input") !== "input") &&
        !useRecordingStore.getState().state.active &&
        !useRecordingStore.getState().busy
      )
    },
    checked: (state) => selected(state)?.recording?.armed ?? false,
    run: async () => {
      const id = useUiStore.getState().selectedTrack
      const recording = useProjectStore
        .getState()
        .project.mixer.tracks.find((track) => track.id === id)?.recording
      if (id !== null && recording)
        await patchTrack(id, {
          recording: { ...recording, armed: !recording.armed },
        })
    },
  },
  {
    id: "mixer.recordArmed",
    ...dynamicTitle("Record armed mixer tracks…", {}),
    section: SECTION,
    keywords: "multitrack audio input drums band microphone take",
    enabled: (state) =>
      state.document.project.mixer.tracks.some(
        (track) => track.recording?.armed
      ) && !useRecordingStore.getState().state.active,
    run: async () => {
      useRecordingStore.setState({ armedTracks: true })
      await openRecording()
    },
  },
  {
    id: "mixer.renderSelected",
    ...dynamicTitle("Render selected mixer tracks…", {}),
    section: SECTION,
    keywords: "offline stems export wave audio files",
    enabled: () => renderableMixerTracks(selectedMixerTracks()).length > 0,
    whyDisabled: () => t("Select a mixer insert or Master"),
    run: () => openMixerRender(selectedMixerTracks()),
  },
  {
    id: "mixer.renderArmed",
    ...dynamicTitle("Render armed mixer tracks…", {}),
    section: SECTION,
    keywords: "offline stems export wave disk audio files",
    enabled: (state) =>
      state.document.project.mixer.tracks.some(
        (track) => track.recording?.armed && !track.current
      ),
    whyDisabled: () => t("Arm a mixer insert or Master"),
    run: () =>
      openMixerRender(
        useProjectStore
          .getState()
          .project.mixer.tracks.filter((track) => track.recording?.armed)
      ),
  },
  {
    id: "mixer.addTrack",
    ...dynamicTitle("Add mixer track", {}),
    section: SECTION,
    defaultShortcut: "Alt+M",
    keywords: "new insert bus",
    enabled: (state) =>
      state.document.project.mixer.tracks.filter((track) => !track.current)
        .length < MAX_MIXER_TRACKS,
    run: addTrack,
  },
  {
    id: "mixer.renameTrack",
    ...dynamicTitle("Rename mixer track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "F2",
    keywords: "name",
    enabled: (state) => selected(state) !== undefined,
    run: onSelected(startRename),
  },
  {
    id: "mixer.changeColor",
    ...dynamicTitle("Change track color…", {}),
    section: SECTION,
    keywords: "colour swatch",
    enabled: (state) => selected(state) !== undefined,
    run: onSelected(startColoring),
  },
  {
    id: "mixer.deleteTrack",
    ...dynamicTitle("Delete mixer track", {}),
    section: SECTION,
    scope: "mixer",
    editCommand: "delete",
    defaultShortcut: "Delete",
    keywords: "remove",
    enabled: (state) => selectedInsert(state) !== undefined,
    run: onSelected(deleteTrack),
  },
  {
    id: "mixer.toggleMute",
    ...dynamicTitle("Mute or unmute track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "M",
    keywords: "silence",
    enabled: (state) => selected(state) !== undefined,
    checked: (state) => selected(state)?.muted ?? false,
    run: onSelected(toggleMute),
  },
  {
    id: "mixer.toggleSolo",
    ...dynamicTitle("Solo or unsolo track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "S",
    keywords: "isolate",
    enabled: (state) =>
      selectedInsert(state) !== undefined && !selectedInsert(state)?.current,
    checked: (state) => selectedInsert(state)?.solo ?? false,
    run: onSelected(toggleSolo),
  },
  {
    id: "mixer.unmuteAll",
    ...dynamicTitle("Unmute all tracks", {}),
    section: SECTION,
    enabled: (state) =>
      state.document.project.mixer.tracks.some((track) => track.muted),
    run: unmuteAll,
  },
  {
    id: "mixer.resetLevels",
    ...dynamicTitle("Reset levels", {}),
    section: SECTION,
    enabled: (state) =>
      state.document.project.mixer.tracks.some(
        (track) => track.volume !== 1 || track.pan !== 0
      ),
    run: resetLevels,
  },
  {
    id: "mixer.selectRoutedHere",
    ...dynamicTitle("Select tracks routed here", {}),
    section: SECTION,
    enabled: (state) => {
      const track = selected(state)
      return (
        track !== undefined &&
        routedTrackIds(state.document.project.mixer.tracks, track.id).length > 0
      )
    },
    run: onSelected((id) => {
      const ids = routedTrackIds(
        useProjectStore.getState().project.mixer.tracks,
        id
      )
      const first = ids[0]
      if (first === undefined) return
      useMixerUi.setState({ selected: ids, anchor: first })
      useUiStore.getState().selectTrack(first)
    }),
  },
  {
    id: "mixer.selectMutedTracks",
    ...dynamicTitle("Select muted tracks", {}),
    section: SECTION,
    enabled: (state) =>
      mutedTrackIds(state.document.project.mixer.tracks).length > 0,
    run: () => {
      const ids = mutedTrackIds(
        visualMixerOrder(useProjectStore.getState().project.mixer.tracks)
      )
      const first = ids[0]
      if (first === undefined) return
      useMixerUi.setState({ selected: ids, anchor: first })
      useUiStore.getState().selectTrack(first)
    },
  },
  {
    id: "mixer.selectSoloTracks",
    ...dynamicTitle("Select solo tracks", {}),
    section: SECTION,
    enabled: (state) =>
      soloTrackIds(state.document.project.mixer.tracks).length > 0,
    run: () => {
      const ids = soloTrackIds(
        visualMixerOrder(useProjectStore.getState().project.mixer.tracks)
      )
      const first = ids[0]
      if (first === undefined) return
      useMixerUi.setState({ selected: ids, anchor: first })
      useUiStore.getState().selectTrack(first)
    },
  },
  {
    id: "mixer.unsoloAll",
    ...dynamicTitle("Unsolo all tracks", {}),
    section: SECTION,
    enabled: (state) =>
      state.document.project.mixer.tracks.some((track) => track.solo),
    run: unsoloAll,
  },
  {
    id: "mixer.resetVolume",
    ...dynamicTitle("Reset fader to 0 dB", {}),
    section: SECTION,
    keywords: "volume level gain unity",
    enabled: (state) => (selected(state)?.volume ?? 1) !== 1,
    run: onSelected(resetVolume),
  },
  {
    id: "mixer.centerPan",
    ...dynamicTitle("Center pan", {}),
    section: SECTION,
    keywords: "reset balance",
    enabled: (state) => (selected(state)?.pan ?? 0) !== 0,
    run: onSelected(centerPan),
  },
  {
    id: "mixer.routeToMaster",
    ...dynamicTitle("Route to master", {}),
    section: SECTION,
    keywords: "output reset routing",
    enabled: (state) => {
      const track = selectedInsert(state)
      return (
        track !== undefined && !track.current && track.output !== MASTER_TRACK
      )
    },
    run: onSelected((id) => setOutput(id, MASTER_TRACK)),
  },
  {
    id: "mixer.resetPeaks",
    ...dynamicTitle("Reset peak readouts", {}),
    section: SECTION,
    keywords: "clip clear meters",
    run: resetAllPeaks,
  },
  {
    id: "mixer.selectNext",
    ...dynamicTitle("Select next mixer track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "ArrowRight",
    repeats: true,
    run: () => moveSelection(1),
  },
  {
    id: "mixer.selectPrevious",
    ...dynamicTitle("Select previous mixer track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "ArrowLeft",
    repeats: true,
    run: () => moveSelection(-1),
  },
  {
    id: "mixer.selectFirst",
    ...dynamicTitle("Select the master track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "Home",
    run: () => moveSelection("first"),
  },
  {
    id: "mixer.selectLast",
    ...dynamicTitle("Select the last mixer track", {}),
    section: SECTION,
    scope: "mixer",
    defaultShortcut: "End",
    run: () => moveSelection("last"),
  },
]

let holders = 0
let unregister: (() => void) | null = null

// The registry survives a hot module replacement, but these module-local
// holders do not. Release the old batch and its subscriptions before the new
// module registers the replacement actions. Existing holder cleanup remains
// safe and cannot remove the new module's registration.
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    unregister?.()
    unregister = null
  })
}

/**
 * Puts the mixer's actions in the registry. The app registers them at boot
 * and the panel does too, so they are there whichever comes first; they
 * leave the registry when the last holder lets go.
 */
export function registerMixerActions(): () => void {
  holders += 1
  if (unregister === null) {
    const remove = registry.register([...MIXER_ACTIONS, ...EFFECT_ACTIONS])
    // The effect actions follow which effect is selected, and "Show
    // effects" whether the inspector is open; neither is in the app state.
    const unfollow = invalidateActionsOn(useEffectsUi, (state) => [
      state.selectedEffect,
      state.inspectorOpen,
    ])
    const unwatch = keepEffectOnSelectedTrack()
    const unrecord = invalidateActionsOn(useRecordingStore, (state) => [
      state.state.active,
      state.busy,
    ])
    const unmixer = invalidateActionsOn(useMixerUi, (state) => state.selected)
    unregister = () => {
      remove()
      unfollow()
      unwatch()
      unrecord()
      unmixer()
    }
  }
  let released = false
  return () => {
    if (released) return
    released = true
    holders -= 1
    if (holders === 0) {
      unregister?.()
      unregister = null
    }
  }
}
