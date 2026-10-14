import { useEffect } from "react"

import { watchAutomatedEdits } from "@/features/automation/notice"
import { connectLanguage, message, translatedText, useLanguageStore, type Language } from "@/lib/i18n"
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
import { useProjectStore } from "@/lib/store/project"

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
      rememberLanguage(),
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

/**
 * Tells the shell the interface language now and whenever it changes. The
 * shell keeps it in the app's settings, where Windfall running as a plugin
 * inside another program reads it for its panel.
 */
export function rememberLanguage(): () => void {
  const remember = (language: Language) => {
    // Not something the user did or can fix, so it is logged, not shown.
    backend.rememberLanguage(language).catch((error: unknown) => {
      console.warn("Could not remember the interface language:", errorMessage(error))
    })
  }
  remember(useLanguageStore.getState().language)
  return useLanguageStore.subscribe((state, before) => {
    if (state.language !== before.language) remember(state.language)
  })
}

/** Puts the project name in the window title, with a mark for unsaved edits. */
export function useWindowTitle(panelTitle?: string) {
  const name = useProjectName()
  const dirty = useDirty()
  const language = useLanguageStore((state) => state.language)
  const unnamed = useProjectStore((state) => state.path === null && !state.project.settings.name)

  useEffect(() => {
    const title = panelTitle
      ? message("{panel} - Windfall", { panel: translatedText(panelTitle, language) }, language)
      : message("{name}{mark} - Windfall", { name: unnamed ? translatedText("Untitled", language) : name, mark: dirty ? "*" : "" }, language)
    // Not something the user did or can fix, so it is logged, not shown.
    backend.setWindowTitle(title).catch((error: unknown) => {
      console.warn("Could not set the window title:", errorMessage(error))
    })
  }, [name, dirty, unnamed, panelTitle, language])
}
