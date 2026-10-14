import { isValidElement } from "react"

import { installKeymap } from "@/lib/actions"
import { registerBuiltinActions } from "@/lib/actions/builtin"
import { setBackend } from "@/lib/ipc"
import {
  createMockBackend,
  type MockBackend,
  type MockDialogs,
  type MockOptions,
} from "@/lib/ipc/mock"
import { localizeBackendText } from "@/lib/i18n"
import { connectStores } from "@/lib/store/connect"
import { useEngineStore } from "@/lib/store/engine"
import { useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { useWarningsStore } from "@/lib/store/warnings"

export const TEST_DIALOGS: MockDialogs = {
  openProject: (saved) => Promise.resolve(saved[0] ?? null),
  saveProject: (suggested) => Promise.resolve(suggested),
  exportPath: (suggested) => Promise.resolve(suggested),
  folder: () => Promise.resolve("/samples/Test"),
  audioFile: () => Promise.resolve("/factory/Drums/Kicks/Kick 02.wav"),
}

/**
 * The words a toast's title or description shows: a coded backend message is
 * handed to the toast as an element that says it in the language shown.
 */
export function toastWords(content: unknown): string {
  if (isValidElement<{ source?: unknown }>(content) && typeof content.props.source === "string")
    return localizeBackendText(content.props.source)
  return String(content)
}

/** Lets promises that are already resolved run their callbacks. */
export async function settle() {
  for (let turn = 0; turn < 10; turn += 1) await Promise.resolve()
}

/**
 * A fresh mock backend with the stores, the actions and the keymap wired to
 * it, the way the app starts. Call the returned `stop` when the test is done.
 */
export async function startTestApp(options: MockOptions = {}) {
  const backend: MockBackend = createMockBackend({
    storage: null,
    dialogs: TEST_DIALOGS,
    ...options,
  })
  setBackend(backend)
  useProjectStore.setState(useProjectStore.getInitialState(), true)
  useTransportStore.setState(useTransportStore.getInitialState(), true)
  useEngineStore.setState(useEngineStore.getInitialState(), true)
  useUiStore.setState(useUiStore.getInitialState(), true)
  usePromptStore.setState(usePromptStore.getInitialState(), true)
  useWarningsStore.setState(useWarningsStore.getInitialState(), true)
  const disconnect = connectStores()
  const unregister = registerBuiltinActions()
  const uninstall = installKeymap()
  await settle()
  return {
    backend,
    stop() {
      disconnect()
      unregister()
      uninstall()
      backend.dispose()
    },
  }
}
