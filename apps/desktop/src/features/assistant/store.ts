import { create } from "zustand"
import { onProjectReplaced } from "@/lib/store/replaced"

export const useAssistantWindows = create<{
  assistant: boolean
  scripting: boolean
  epoch: number
}>(() => ({ assistant: false, scripting: false, epoch: 0 }))

export function openAssistant() { useAssistantWindows.setState({ assistant: true }) }
export function openScripts() { useAssistantWindows.setState({ scripting: true }) }
export function closeAssistant() { useAssistantWindows.setState({ assistant: false }) }
export function closeScripts() { useAssistantWindows.setState({ scripting: false }) }
onProjectReplaced(() => useAssistantWindows.setState((state) => ({ assistant: false, scripting: false, epoch: state.epoch + 1 })))
