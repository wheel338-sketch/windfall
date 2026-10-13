import { registerDynamicActionMessage, removeDynamicActionMessage, type DynamicActionMessage } from "@/lib/i18n/action-messages"
import type { ChannelId, TrackId, TransportState } from "@/bindings"
import type { ProjectState } from "@/lib/store/project"
import type {
  CenterOverlay,
  CenterTab,
  KeymapPreset,
  ScopeId,
  SidePanel,
  Theme,
} from "@/lib/store/ui"

/** What an action may look at to decide whether it is enabled or checked. */
export type AppState = {
  document: ProjectState
  transport: TransportState
  ui: {
    theme: Theme
    keymap: KeymapPreset
    panels: Record<SidePanel, boolean>
    centerTab: CenterTab
    centerOverlay: CenterOverlay | null
    selectedChannel: ChannelId | null
    selectedTrack: TrackId | null
    /** The scope that has the keyboard: the one last clicked or focused. */
    activeScope: ScopeId
  }
}

/**
 * Where an action's shortcuts are live. A panel's own actions name the
 * panel, or a part of it; everything else is "global".
 */
export type ShortcutScope = "global" | ScopeId

/** The commands of the Edit menu that mean something else in every panel. */
export type EditCommand =
  "cut" | "copy" | "paste" | "duplicate" | "delete" | "selectAll"

/**
 * One thing the user can do. Menus, the command palette, context menus and
 * the keymap are all built from these; nothing binds a key or builds a menu
 * item any other way.
 */
export type Action = {
  /** Stable name, such as `file.save`. Keymaps refer to it. */
  id: string
  /** What menus and the palette show. Sentence case, no trailing dots. */
  title: string
  titleMessage?: DynamicActionMessage
  /** Groups actions in the palette: File, Edit, Transport, View and so on. */
  section: string
  /**
   * Shortcut in the Windfall keymap, such as `Mod+Shift+Z`. `Mod` is Ctrl on
   * Windows and Linux and Cmd on macOS. Several shortcuts may be given.
   */
  defaultShortcut?: string | string[]
  /**
   * The panel the shortcuts belong to. They fire only while that panel has
   * the keyboard, so two panels can give the same key to different actions.
   * Leave out for a shortcut that works everywhere. The action itself can
   * still be run from anywhere, by a menu or the palette.
   */
  scope?: ScopeId
  /**
   * Makes this the action that Edit > Cut, Copy, Delete and so on run while
   * the action's panel has the keyboard. Needs a `scope`.
   */
  editCommand?: EditCommand
  /**
   * For an action that only passes the work on, as Edit > Delete does: the
   * id of the action it runs right now. Its shortcut is shown for both.
   */
  standsFor?(state: AppState): string | undefined
  /** Extra words the palette search matches. */
  keywords?: string
  /** Runs again and again while the key is held, as undo does. */
  repeats?: boolean
  /**
   * Leave out for an action that is always available. State that is not in
   * `AppState` may be read here too, from the panel's own store, as long as
   * the panel calls `registry.invalidate()` when that state changes.
   */
  enabled?(state: AppState): boolean
  /**
   * A few words on why the action cannot run right now, such as "Samplers
   * only". Menus and the palette show them beside the greyed-out title.
   * Asked only while the action is disabled.
   */
  whyDisabled?(state: AppState): string | undefined
  /** For actions that are on or off, such as "Show browser". */
  checked?(state: AppState): boolean
  run(): void | Promise<void>
}

/**
 * A keymap preset's shortcuts by action id, in place of the actions' own.
 * `null` leaves an action without a key.
 */
export type PresetShortcuts = Record<string, string[] | null>

export type RegisterOptions = {
  /** Shortcuts these actions have in the other keymap presets. */
  presets?: Partial<Record<KeymapPreset, PresetShortcuts>>
}

export type Registry = {
  /** Adds actions. Returns a function that removes them again. */
  register(actions: Action[], options?: RegisterOptions): () => void
  get(id: string): Action | undefined
  /** Every action, in the order registered. The array changes when the set does. */
  list(): Action[]
  /** A preset's shortcuts for the registered actions. Changes when the set does. */
  presetShortcuts(preset: KeymapPreset): PresetShortcuts
  subscribe(listener: () => void): () => void
  /**
   * Says that state actions read outside `AppState` has changed, so that
   * buttons and open menus work out `enabled` and `checked` again.
   */
  invalidate(): void
  /** Goes up with every `invalidate`. */
  stateVersion(): number
  subscribeState(listener: () => void): () => void
}

export function createRegistry(): Registry {
  const actions = new Map<string, Action>()
  const batches = new Set<RegisterOptions>()
  const listeners = new Set<() => void>()
  const stateListeners = new Set<() => void>()
  let snapshot: Action[] = []
  let presets = new Map<KeymapPreset, PresetShortcuts>()
  let version = 0

  function changed() {
    snapshot = [...actions.values()]
    presets = new Map()
    for (const listener of listeners) listener()
  }

  return {
    register(added, options = {}) {
      for (const action of added) {
        if (actions.has(action.id)) {
          throw new Error(`Action "${action.id}" is registered twice`)
        }
        if (action.editCommand !== undefined && action.scope === undefined) {
          throw new Error(
            `Action "${action.id}" has an edit command but no scope`
          )
        }
      }
      const ids = new Set(added.map((action) => action.id))
      for (const shortcuts of Object.values(options.presets ?? {})) {
        for (const id of Object.keys(shortcuts)) {
          if (!ids.has(id)) {
            throw new Error(
              `A keymap preset names "${id}", which is not among the actions it was registered with`
            )
          }
        }
      }
      for (const action of added) { actions.set(action.id, action); registerDynamicActionMessage(action.id, action, action.titleMessage) }
      batches.add(options)
      changed()
      return () => {
        for (const action of added) {
          if (actions.get(action.id) === action) { actions.delete(action.id); removeDynamicActionMessage(action.id, action) }
        }
        batches.delete(options)
        changed()
      }
    },
    get: (id) => actions.get(id),
    list: () => snapshot,
    presetShortcuts(preset) {
      let merged = presets.get(preset)
      if (!merged) {
        merged = {}
        for (const batch of batches) {
          Object.assign(merged, batch.presets?.[preset])
        }
        presets.set(preset, merged)
      }
      return merged
    },
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    invalidate() {
      version += 1
      for (const listener of stateListeners) listener()
    },
    stateVersion: () => version,
    subscribeState(listener) {
      stateListeners.add(listener)
      return () => {
        stateListeners.delete(listener)
      }
    },
  }
}

export function isEnabled(action: Action, state: AppState): boolean {
  return action.enabled ? action.enabled(state) : true
}

/** Why a disabled action is disabled, when it says. */
export function disabledReason(
  action: Action,
  state: AppState
): string | undefined {
  return isEnabled(action, state) ? undefined : action.whyDisabled?.(state)
}

export function isChecked(action: Action, state: AppState): boolean {
  return action.checked ? action.checked(state) : false
}

/** The app's one registry. */
export const registry = createRegistry()
