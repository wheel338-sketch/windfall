import { useSyncExternalStore } from "react"
import { useShallow } from "zustand/react/shallow"

import { reportError } from "@/lib/errors"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { activeScopeOf, useUiStore } from "@/lib/store/ui"

import {
  detectMac,
  eventChord,
  formatShortcut,
  isTransportChord,
  resolveInChain,
  resolveKeymap,
  shortcutAllowed,
  type Keymap,
} from "./keymap"
import {
  isEnabled,
  registry,
  type Action,
  type AppState,
  type PresetShortcuts,
} from "./registry"
import { installScopeTracking, scopeLinks } from "./scope"
import { message } from "@/lib/i18n"

export { registry, disabledReason, isEnabled, isChecked } from "./registry"
export type {
  Action,
  AppState,
  EditCommand,
  PresetShortcuts,
  ShortcutScope,
} from "./registry"
export { INSPECTOR_KEEPS, useShortcutScope } from "./scope"

const isMac = detectMac()

function uiSlice(
  state: ReturnType<typeof useUiStore.getState>
): AppState["ui"] {
  return {
    theme: state.theme,
    keymap: state.keymap,
    panels: state.panels,
    centerTab: state.centerTab,
    centerOverlay: state.centerOverlay,
    selectedChannel: state.selectedChannel,
    selectedTrack: state.selectedTrack,
    activeScope: activeScopeOf(state),
  }
}

/** The state actions look at, read right now. */
export function getAppState(): AppState {
  return {
    document: useProjectStore.getState(),
    transport: useTransportStore.getState(),
    ui: uiSlice(useUiStore.getState()),
  }
}

function subscribeAppState(listener: () => void): () => void {
  const stops = [
    useProjectStore.subscribe(listener),
    useTransportStore.subscribe(listener),
    useUiStore.subscribe(listener),
    registry.subscribeState(listener),
  ]
  return () => {
    for (const stop of stops) stop()
  }
}

/**
 * The state actions look at, kept current. It changes on every edit, so use
 * it only in parts of the UI that are open briefly, such as an open menu.
 */
export function useAppState(): AppState {
  useProjectStore()
  useTransportStore()
  useUiStore(useShallow(uiSlice))
  useSyncExternalStore(registry.subscribeState, registry.stateVersion)
  return getAppState()
}

/**
 * Derives a value from the state actions look at, including what panels
 * report through `registry.invalidate()`. The component renders again only
 * when the derived value changes, so keep it a primitive.
 */
export function useAppSelector<T>(select: (state: AppState) => T): T {
  return useSyncExternalStore(subscribeAppState, () => select(getAppState()))
}

/** Whether an action can run right now. False for an unknown id. */
export function useActionEnabled(id: string): boolean {
  const action = useAction(id)
  return useAppSelector((state) => (action ? isEnabled(action, state) : false))
}

/**
 * Makes actions follow a panel's own store. Whenever what `pick` returns
 * changes, buttons and open menus work out `enabled` and `checked` again.
 * `pick` returns the values the panel's actions read, as a list:
 *
 *     invalidateActionsOn(useRackStore, (state) => [state.inspectorOpen])
 */
export function invalidateActionsOn<S>(
  store: { subscribe(listener: (state: S, previous: S) => void): () => void },
  pick: (state: S) => readonly unknown[]
): () => void {
  return store.subscribe((state, previous) => {
    const before = pick(previous)
    if (pick(state).some((value, index) => !Object.is(value, before[index]))) {
      registry.invalidate()
    }
  })
}

/** Runs an action by id, unless it is disabled. Failures are shown. */
export async function runAction(id: string): Promise<void> {
  const action = registry.get(id)
  if (!action) {
    reportError(new Error(message("There is no action called \"{id}\".", { id })))
    return
  }
  if (!isEnabled(action, getAppState())) return
  try {
    await action.run()
  } catch (error) {
    reportError(error, action.titleMessage ?? action.title)
  }
}

/** Every registered action. Renders again when actions are added or removed. */
export function useActions(): Action[] {
  return useSyncExternalStore(registry.subscribe, registry.list)
}

export function useAction(id: string): Action | undefined {
  return useActions().find((action) => action.id === id)
}

let cached: {
  actions: Action[]
  overrides: PresetShortcuts
  keymap: Keymap
} | null = null

/** The keymap for the chosen preset. Rebuilt only when it can have changed. */
export function currentKeymap(): Keymap {
  const actions = registry.list()
  const overrides = registry.presetShortcuts(useUiStore.getState().keymap)
  if (cached?.actions !== actions || cached.overrides !== overrides) {
    cached = {
      actions,
      overrides,
      keymap: resolveKeymap(actions, overrides, isMac),
    }
  }
  return cached.keymap
}

/**
 * An action's main shortcut, written for this platform, or undefined. It is
 * the key that runs the action inside the action's own scope.
 */
export function shortcutLabel(id: string): string | undefined {
  const action = registry.get(id)
  const target = action?.standsFor?.(getAppState()) ?? id
  const shortcut = currentKeymap().byAction.get(target)?.[0]
  return shortcut ? formatShortcut(shortcut, isMac) : undefined
}

/** Like `shortcutLabel`, and follows keymap changes. */
export function useShortcutLabel(id: string): string | undefined {
  useActions()
  return useAppSelector(() => shortcutLabel(id))
}

/**
 * What a key press comes to right now: the action it runs, or that a scope
 * around the focus keeps the key from the action it would have run.
 */
function resolveEvent(event: KeyboardEvent): {
  action?: Action
  kept: boolean
} {
  const chord = eventChord(event)
  if (chord === null || !shortcutAllowed(event.target, chord)) {
    return { kept: false }
  }
  const state = getAppState()
  const resolved = resolveInChain(
    currentKeymap(),
    chord,
    scopeLinks(event.target),
    (candidate) => {
      const action = registry.get(candidate)
      return (
        action && {
          enabled: isEnabled(action, state),
          editCommand: action.editCommand,
        }
      )
    }
  )
  if (resolved === undefined) return { kept: false }
  if ("kept" in resolved) return { kept: true }
  return { action: registry.get(resolved.action), kept: false }
}

/** The action a key press runs right now, if any. */
export function actionForEvent(event: KeyboardEvent): Action | undefined {
  return resolveEvent(event).action
}

function isTransportKey(event: KeyboardEvent): boolean {
  const chord = eventChord(event)
  return chord !== null && isTransportChord(chord)
}

let installed: { holders: number; uninstall(): void } | null = null

function listen(): () => void {
  // Transport keys are taken on the way down to the focused control, before
  // a button or a slider can use them for itself.
  const onTransportKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing || !isTransportKey(event)) return
    const action = actionForEvent(event)
    if (!action) return
    event.preventDefault()
    event.stopPropagation()
    if (event.repeat && !action.repeats) return
    void runAction(action.id)
  }
  // A button clicks itself when Space comes back up, so that half is kept
  // from it too.
  const onTransportKeyUp = (event: KeyboardEvent) => {
    if (!isTransportKey(event) || !actionForEvent(event)) return
    event.preventDefault()
    event.stopPropagation()
  }
  // Every other key reaches the focused control first. A control that uses
  // the key marks the event as handled, and then the keymap leaves it alone.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.defaultPrevented || event.isComposing) return
    if (isTransportKey(event)) return
    const { action, kept } = resolveEvent(event)
    // A key an inspector keeps from its panel is used up, and does nothing.
    if (kept) event.preventDefault()
    // A key with no action that can run right now is not ours to cancel.
    if (!action) return
    // Stops the webview's own handling, such as Ctrl+S saving the page.
    event.preventDefault()
    if (event.repeat && !action.repeats) return
    void runAction(action.id)
  }
  const stopTracking = installScopeTracking()
  window.addEventListener("keydown", onTransportKeyDown, true)
  window.addEventListener("keyup", onTransportKeyUp, true)
  window.addEventListener("keydown", onKeyDown)
  return () => {
    stopTracking()
    window.removeEventListener("keydown", onTransportKeyDown, true)
    window.removeEventListener("keyup", onTransportKeyUp, true)
    window.removeEventListener("keydown", onKeyDown)
  }
}

/**
 * Listens for shortcuts on the window, and follows which panel has the
 * keyboard. Returns a function that stops. Installing twice listens once.
 */
export function installKeymap(): () => void {
  installed ??= { holders: 0, uninstall: listen() }
  installed.holders += 1
  let released = false
  return () => {
    if (released || !installed) return
    released = true
    installed.holders -= 1
    if (installed.holders > 0) return
    installed.uninstall()
    installed = null
  }
}
