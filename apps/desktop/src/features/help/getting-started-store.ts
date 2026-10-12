import { create } from "zustand"

import { onProjectReplaced } from "@/lib/store/replaced"
import type { TutorialKind } from "./tutorials"
import type { PanelId } from "@/lib/store/ui"

// Separate from the shared dialog slot so Export can open without closing help.
export const useGettingStartedStore = create(() => ({ open: false, tutorial: null as TutorialKind | null, context: undefined as PanelId | undefined }))

let stopReplacement: (() => void) | null = null

export function openGettingStarted() {
  stopReplacement ??= onProjectReplaced(closeGettingStarted)
  useGettingStartedStore.setState({ open: true, context: undefined })
}

export function openContextHelp(context: PanelId) {
  stopReplacement ??= onProjectReplaced(closeGettingStarted)
  useGettingStartedStore.setState({ open: true, context })
}

export function closeGettingStarted() {
  stopReplacement?.()
  stopReplacement = null
  useGettingStartedStore.setState({ open: false, tutorial: null as TutorialKind | null, context: undefined })
}
