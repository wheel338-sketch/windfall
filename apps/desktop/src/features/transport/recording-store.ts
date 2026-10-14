import { create } from "zustand"
import { toast } from "sonner"
import type { RecordingInput, RecordingState } from "@/bindings"
import { backend, errorSource } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { translatedText } from "@/lib/i18n"

export const useRecordingStore = create(() => ({
  open: false,
  busy: false,
  inputs: [] as RecordingInput[],
  device: 0,
  left: 0,
  right: -1,
  start: 0,
  track: -1,
  synchronize: true,
  driftCorrection: true,
  offsetMs: 0,
  inputRate: 0,
  loopRecording: false,
  loopEnd: 3840,
  keepLatest: false,
  excludedTakes: [] as number[],
  monitor: false,
  monitorTrack: 0,
  monitorGain: 0.5,
  monitorBufferMs: 20,
  armedTracks: false,
  state: {
    active: false,
    frames: 0,
    sampleRate: 0,
    startTick: 0,
    error: null,
  } as RecordingState,
  error: "",
}))
async function work(action: () => Promise<void>) {
  if (useRecordingStore.getState().busy) return
  useRecordingStore.setState({ busy: true, error: "" })
  try {
    await action()
  } catch (error) {
    useRecordingStore.setState({ error: errorSource(error) })
  } finally {
    useRecordingStore.setState({ busy: false })
    await refreshRecording()
  }
}
export async function refreshRecording() {
  try {
    useRecordingStore.setState({ state: await backend.recordingState() })
  } catch (error) {
    useRecordingStore.setState({ error: errorSource(error) })
  }
}
export async function openRecording() {
  useRecordingStore.setState({ open: true })
  await work(async () => {
    useRecordingStore.setState({ inputs: await backend.recordingInputs() })
  })
}
export function startRecording() {
  return work(async () => {
    const s = useRecordingStore.getState()
    const device = s.inputs[s.device]
    if (!device && !s.armedTracks) throw new Error(translatedText("Select a native audio input."))
    const state = await backend.recordingStart(
      {
        host: device?.host ?? "",
        device: device?.device ?? "",
        left: s.left,
        right: s.right < 0 ? null : s.right,
        alignment: {
          synchronize: s.synchronize,
          driftCorrection: s.driftCorrection,
          offsetMs: s.offsetMs,
          inputSampleRate: s.inputRate === 0 ? null : s.inputRate,
        },
        loopRecording: s.loopRecording ? { region: { start: s.start, end: s.loopEnd } } : undefined,
        monitor: s.monitor ? { track: s.monitorTrack, gain: s.monitorGain, bufferMs: s.monitorBufferMs } : undefined,
        armedTracks: s.armedTracks,
      },
      s.start,
      s.track < 0 ? null : s.track
    )
    useRecordingStore.setState({ state, excludedTakes: [] })
  })
}
export function stopRecording() {
  return work(async () => {
    const generation = getProjectGeneration()
    const s = useRecordingStore.getState()
    const result = await backend.recordingStop(s.state.takes
      ? s.keepLatest ? { type: "latest" } : { type: "except", indices: s.excludedTakes }
      : undefined)
    if (generation !== getProjectGeneration())
      throw new Error(translatedText("The project changed while the take was being attached. Reopen recording."))
    receivePatch(result.patch)
    toast.success(translatedText("Recording added to playlist"))
  })
}
export function cancelRecording() {
  return work(async () => {
    await backend.recordingCancel()
    toast(translatedText("Recording discarded"))
  })
}
