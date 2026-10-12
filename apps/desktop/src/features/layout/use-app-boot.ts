import { useEffect } from "react"

import { watchAutomatedEdits } from "@/features/automation/notice"
import { connectLanguage } from "@/lib/i18n"
import { connectNativeEditorScale } from "@/features/plugins/editor-scale"
import { installKeymap } from "@/lib/actions"
import { syncRecentActions } from "@/lib/actions/builtin"
import {
  confirmDiscardChanges,
  refreshRecentProjects,
  useRecentStore,
} from "@/lib/flows/project"
import { backend, errorMessage } from "@/lib/ipc"
import { connectStores } from "@/lib/store/connect"
import { installTextFieldMenu } from "@/lib/text-field-menu"
import { useDirty, useProjectName } from "@/lib/store/selectors"
import { installWebviewGuard } from "@/lib/webview-guard"

import { registerAllActions } from "./register-actions"

type BootOptions = {
  /** The main window asks before closing with unsaved edits. */
  guardClose: boolean
}

/**
 * Starts everything a window needs: the stores follow the backend, the
 * actions are registered and the keymap listens. Call once at the top of a
 * window.
 */
export function useAppBoot({ guardClose }: BootOptions) {
  useEffect(() => {
    const stop = [
      connectLanguage(),
      connectNativeEditorScale(),
      connectStores(),
      registerAllActions(),
      installKeymap(),
      installTextFieldMenu(),
      watchAutomatedEdits(),
      useRecentStore.subscribe((state) => syncRecentActions(state.paths)),
    ]
    // In development the webview's reload, menu and tools are wanted.
    if (!import.meta.env.DEV) stop.push(installWebviewGuard())
    if (guardClose) stop.push(backend.onCloseRequested(confirmDiscardChanges))
    void refreshRecentProjects()
    return () => {
      for (const off of stop) off()
    }
  }, [guardClose])
}

/** Puts the project name in the window title, with a mark for unsaved edits. */
export function useWindowTitle() {
  const name = useProjectName()
  const dirty = useDirty()

  useEffect(() => {
    const title = `${name}${dirty ? "*" : ""} - Windfall`
    // Not something the user did or can fix, so it is logged, not shown.
    backend.setWindowTitle(title).catch((error: unknown) => {
      console.warn("Could not set the window title:", errorMessage(error))
    })
  }, [name, dirty])
}
