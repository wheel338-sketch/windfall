import { create } from "zustand"

import { samePoints, toCurveTick } from "@/features/playlist/automation/points"
import { rangeNormalized } from "@/lib/automation/curve"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { targetState } from "@/lib/automation/targets"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { automatedValue, realtimeFrame } from "@/lib/store/realtime"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useTransportStore } from "@/lib/store/transport"
import { MAX_SONG_TICKS } from "@/lib/units"

import { editedWhileAutomated } from "./notice"
import { drivingAutomation } from "./now"

/** A session arm, separate from audio recording and never persisted. */
export const useAutomationRecordStore = create<{ recordArmed: boolean }>(
  () => ({
    recordArmed: false,
  })
)

onProjectReplaced(() => {
  useAutomationRecordStore.setState({ recordArmed: false })
})

// In the desktop app the shell records: it sees controller movements as
// well as edits made here, and writes the pass as one undo step when the
// transport stops. The arm is the same button.
useAutomationRecordStore.subscribe((state, previous) => {
  if (backend.kind !== "tauri" || state.recordArmed === previous.recordArmed)
    return
  backend
    .automationRecordArm(state.recordArmed)
    .catch((error: unknown) =>
      reportError(error, "Could not change automation recording")
    )
})

/** Records committed control edits into an existing clip while playing. */
export function watchAutomationRecording(): () => void {
  let writing = false
  return useProjectStore.subscribe((state, previous) => {
    if (
      // The shell does the recording in the desktop app.
      backend.kind === "tauri" ||
      writing ||
      !useAutomationRecordStore.getState().recordArmed ||
      !useTransportStore.getState().playing
    )
      return

    const name = editedWhileAutomated(previous.project, state.project)
    if (name === null) return
    // Names need not be unique: also identify the target that changed.
    const edited = state.project.automations.find((automation) => {
      if (
        automation.name !== name ||
        automatedValue(automation.id) === undefined
      )
        return false
      const was = targetState(previous.project, automation.target)
      const now = targetState(state.project, automation.target)
      return was && now && was.stored !== now.stored
    })
    if (!edited) return
    const driving = drivingAutomation(edited.target)
    if (!driving) return
    const { automation } = driving
    const songTick = Math.floor(realtimeFrame().tick)
    const clip = state.project.playlist.clips
      .filter(
        (item) =>
          item.content.type === "automation" &&
          item.content.automation === automation.id &&
          item.start <= songTick &&
          songTick < item.start + item.length
      )
      .sort((a, b) => a.start - b.start)[0]
    if (!clip) return
    const tick = toCurveTick(clip, songTick)
    if (!Number.isFinite(tick) || tick < 0 || tick > MAX_SONG_TICKS) return
    const target = targetState(state.project, automation.target)
    if (!target) return
    const value = Math.min(
      1,
      Math.max(0, rangeNormalized(target.range, target.stored))
    )
    const points = automation.points.filter((point) => point.tick !== tick)
    const index = points.findIndex((point) => point.tick > tick)
    points.splice(index < 0 ? points.length : index, 0, {
      tick,
      value,
      curve: 0,
      hold: false,
    })
    if (samePoints(points, automation.points)) return

    // The command's patch notifies this subscriber too.
    writing = true
    void dispatch({
      type: "setAutomationPoints",
      id: automation.id,
      points,
    }).finally(() => {
      writing = false
    })
  })
}
