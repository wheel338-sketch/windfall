import { create } from "zustand"
import type { InputMonitorState } from "@/bindings"
import { backend, errorSource } from "@/lib/ipc"

export const useInputMonitorStore = create(() => ({
  busy: false,
  error: "",
  state: { active: false, sampleRate: 0, tracks: [], error: null } as InputMonitorState,
}))
let timer: ReturnType<typeof setInterval> | undefined
let refreshing = false
function receive(state: InputMonitorState) {
  useInputMonitorStore.setState({ state, error: state.error ?? "" })
  if (state.active && !timer) timer = setInterval(() => void refreshInputMonitors(), 250)
  if (!state.active && timer) { clearInterval(timer); timer = undefined }
}
export async function refreshInputMonitors() {
  if (refreshing || useInputMonitorStore.getState().busy) return
  refreshing = true
  try { receive(await backend.inputMonitorState()) }
  catch (error) { useInputMonitorStore.setState({ error: errorSource(error) }) }
  finally { refreshing = false }
}
export async function toggleInputMonitors() {
  if (useInputMonitorStore.getState().busy) return
  useInputMonitorStore.setState({ busy: true, error: "" })
  try {
    receive(await (useInputMonitorStore.getState().state.active ? backend.inputMonitorStop() : backend.inputMonitorStart()))
  } catch (error) {
    useInputMonitorStore.setState({ error: errorSource(error) })
    receive(await backend.inputMonitorState().catch(() => ({ active: false, sampleRate: 0, tracks: [], error: errorSource(error) })))
    useInputMonitorStore.setState({ error: errorSource(error) })
  } finally { useInputMonitorStore.setState({ busy: false }) }
}
