import type { KeymapPreset } from "@/lib/store/ui"

import type {
  Action,
  EditCommand,
  PresetShortcuts,
  ShortcutScope,
} from "./registry"
import { message, translatedText as t } from "@/lib/i18n"

/*
 * A shortcut is written as modifiers and one key joined by "+", such as
 * "Mod+Shift+Z". "Mod" is Ctrl on Windows and Linux and Cmd on macOS. Keys
 * are letters, digits, F1 to F24, Space, Comma, Enter, Escape, Delete, the
 * arrows (ArrowUp...) and numpad keys by their code (NumpadAdd...).
 */

/**
 * FL Studio's default shortcuts for the actions Windfall has, taken from the
 * shortcut list in the FL Studio manual. `null` leaves an action without a
 * key: FL uses Ctrl+N for "Save new version", so it must not start a new
 * project. Actions not listed keep their Windfall shortcut when it is free.
 * A panel hands its own FL shortcuts to `registry.register` with its actions.
 */
export const FL_KEYMAP: PresetShortcuts = {
  "file.new": null,
  "file.open": ["Mod+O"],
  "file.save": ["Mod+S"],
  "file.saveAs": ["Mod+Shift+S"],
  "file.export": ["Mod+R"],
  "edit.undo": ["Mod+Alt+Z", "Mod+Z"],
  "transport.toggle": ["Space"],
  "transport.toggleMode": ["L"],
  "pattern.next": ["NumpadAdd"],
  "pattern.previous": ["NumpadSubtract"],
  "pattern.add": ["F4"],
  "pattern.rename": ["F2"],
  "view.playlist": ["F5"],
  "view.channelRack": ["F6"],
  "view.pianoRoll": ["F7"],
  "view.browser": ["Alt+F8"],
  "view.mixer": ["F9"],
  "options.settings": ["F10"],
}

export const KEYMAP_PRESETS: {
  id: KeymapPreset
  name: string
  about: string
}[] = [
  { id: "windfall", name: "Windfall", about: "Shortcuts most apps share." },
  {
    id: "fl",
    name: "FL Studio",
    about: "FL Studio's defaults, for people switching over.",
  },
]

export function detectMac(): boolean {
  return (
    typeof navigator !== "undefined" &&
    /mac|iphone|ipad/i.test(navigator.platform)
  )
}

const MODIFIER_ORDER = ["Ctrl", "Alt", "Shift", "Meta"] as const
type Modifier = (typeof MODIFIER_ORDER)[number]

function chord(modifiers: Set<Modifier>, key: string): string {
  return [...MODIFIER_ORDER.filter((name) => modifiers.has(name)), key].join(
    "+"
  )
}

/** Rewrites a shortcut into the one spelling events are compared against. */
export function normalizeShortcut(shortcut: string, isMac: boolean): string {
  const parts = shortcut.split("+").map((part) => part.trim())
  const key = parts.pop() ?? ""
  const modifiers = new Set<Modifier>()
  for (const part of parts) {
    const name = part.toLowerCase()
    if (name === "mod") modifiers.add(isMac ? "Meta" : "Ctrl")
    else if (name === "ctrl" || name === "control") modifiers.add("Ctrl")
    else if (name === "alt" || name === "option") modifiers.add("Alt")
    else if (name === "shift") modifiers.add("Shift")
    else if (name === "meta" || name === "cmd") modifiers.add("Meta")
    else throw new Error(message("Unknown modifier \"{part}\" in shortcut \"{shortcut}\"", { part, shortcut }))
  }
  return chord(modifiers, key.length === 1 ? key.toUpperCase() : key)
}

type KeyEventLike = Pick<
  KeyboardEvent,
  "key" | "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey"
>

function keyName(event: KeyEventLike): string | null {
  const { key, code } = event
  if (["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(key)) return null
  if (code.startsWith("Numpad")) return code
  if (code.startsWith("Digit")) return code.slice(5)
  if (code === "Space" || key === " ") return "Space"
  if (/^F\d{1,2}$/.test(code)) return code
  if (key === ",") return "Comma"
  // The printed letter wins so shortcuts follow the keyboard layout. With
  // Alt held macOS prints a symbol instead, and then the key's place decides.
  if (/^[a-z]$/i.test(key)) return key.toUpperCase()
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  return key
}

/** The shortcut an event stands for, or null for a bare modifier key. */
export function eventChord(event: KeyEventLike): string | null {
  const key = keyName(event)
  if (key === null) return null
  const modifiers = new Set<Modifier>()
  if (event.ctrlKey) modifiers.add("Ctrl")
  if (event.altKey) modifiers.add("Alt")
  if (event.shiftKey) modifiers.add("Shift")
  if (event.metaKey) modifiers.add("Meta")
  return chord(modifiers, key)
}

const KEY_LABELS: Record<string, string> = {
  Comma: ",",
  get NumpadAdd() { return t("Num +") },
  get NumpadSubtract() { return t("Num -") },
  get ArrowUp() { return t("↑") },
  get ArrowDown() { return t("↓") },
  ArrowLeft: "←",
  ArrowRight: "→",
  get Escape() { return t("Esc") },
  get Delete() { return t("Del") },
}

const MAC_MODIFIERS: Record<Modifier, string> = {
  get Ctrl() { return t("⌃") },
  get Alt() { return t("⌥") },
  get Shift() { return t("⇧") },
  get Meta() { return t("⌘") },
}

/** What the modifier keys are called on a keyboard in the interface language. */
const MODIFIER_NAMES: Record<Modifier, string> = {
  get Ctrl() { return message("key.ctrl") },
  get Alt() { return message("key.alt") },
  get Shift() { return message("key.shift") },
  get Meta() { return message("key.win") },
}

/** A shortcut the way the platform writes it: `Ctrl+Shift+Z` or `⇧⌘Z`. */
export function formatShortcut(shortcut: string, isMac: boolean): string {
  const parts = normalizeShortcut(shortcut, isMac).split("+")
  const key = parts.pop() ?? ""
  // A key name with no entry of its own, such as a letter, is shown as it is.
  const label = t(KEY_LABELS[key] ?? key)
  if (isMac) {
    const symbols = parts
      .map((part) => MAC_MODIFIERS[part as Modifier])
      .join("")
    return [symbols, label].join(label.length > 1 && symbols ? " " : "")
  }
  return [...parts.map((part) => MODIFIER_NAMES[part as Modifier]), label].join(
    "+"
  )
}

function shortcutList(value: string | string[] | undefined): string[] {
  if (value === undefined) return []
  return Array.isArray(value) ? value : [value]
}

export type Keymap = {
  /** For each scope, normalized chord to the id of the action it runs there. */
  byScope: Map<ShortcutScope, Map<string, string>>
  /** Action id to its shortcuts as written, the main one first. */
  byAction: Map<string, string[]>
}

/**
 * Works out which key runs which action in which scope. A key is taken
 * once per scope, so a panel may use a key another panel or the whole app
 * uses too. The preset's own bindings come first; an action the preset does
 * not mention keeps its Windfall shortcut unless the preset already uses
 * that key in the same scope.
 */
export function resolveKeymap(
  actions: Action[],
  overrides: PresetShortcuts,
  isMac: boolean
): Keymap {
  const byScope: Keymap["byScope"] = new Map()
  const byAction = new Map<string, string[]>()

  const bind = (action: Action, shortcuts: string[]) => {
    const scope = action.scope ?? "global"
    const chords = byScope.get(scope) ?? new Map<string, string>()
    byScope.set(scope, chords)
    for (const shortcut of shortcuts) {
      const normalized = normalizeShortcut(shortcut, isMac)
      if (chords.has(normalized)) continue
      chords.set(normalized, action.id)
      byAction.set(action.id, [...(byAction.get(action.id) ?? []), shortcut])
    }
  }

  for (const action of actions) {
    const shortcuts = overrides[action.id]
    if (shortcuts) bind(action, shortcuts)
  }
  for (const action of actions) {
    if (action.id in overrides) continue
    bind(action, shortcutList(action.defaultShortcut))
  }
  return { byScope, byAction }
}

/**
 * The action a key runs: the first one bound to it that can run right now,
 * looking in `scopes` from the innermost outward and in "global" last. A
 * disabled action does not hold on to its key, so the same key can fall
 * through to an outer scope.
 */
export function resolveChord(
  keymap: Keymap,
  chord: string,
  scopes: readonly ShortcutScope[],
  enabled: (id: string) => boolean
): string | undefined {
  for (const scope of [...scopes, "global" as const]) {
    const id = keymap.byScope.get(scope)?.get(chord)
    if (id !== undefined && enabled(id)) return id
  }
  return undefined
}

/** One scope a key passes through, and what it keeps from the ones after it. */
export type ChainLink = {
  scope: ShortcutScope
  keeps: readonly EditCommand[]
}

export type Resolved =
  /** The action the key runs. */
  | { action: string }
  /**
   * The key would have run an Edit command further out that a scope on the
   * way keeps for itself, so it does nothing, and nothing else gets it.
   */
  | { kept: true }
  | undefined

/**
 * Like `resolveChord`, for a chain whose scopes may keep Edit commands from
 * the scopes around them: Delete in a channel's settings must not go on to
 * delete the channel. `describe` says whether an action can run and which
 * Edit command it is, if any.
 */
export function resolveInChain(
  keymap: Keymap,
  chord: string,
  chain: readonly ChainLink[],
  describe: (
    id: string
  ) => { enabled: boolean; editCommand?: EditCommand } | undefined
): Resolved {
  const kept = new Set<EditCommand>()
  const links: ChainLink[] = [...chain, { scope: "global", keeps: [] }]
  for (const link of links) {
    const id = keymap.byScope.get(link.scope)?.get(chord)
    const found = id === undefined ? undefined : describe(id)
    if (id !== undefined && found?.enabled) {
      return found.editCommand !== undefined && kept.has(found.editCommand)
        ? { kept: true }
        : { action: id }
    }
    for (const command of link.keeps) kept.add(command)
  }
  return undefined
}

const TEXT_EDITING_KEYS = new Set(["Z", "Y", "A", "C", "X", "V"])

const TEXT_ENTRY_SELECTOR =
  "input, textarea, select, [contenteditable=''], [contenteditable='true'], [role='textbox'], [role='combobox']"

const OVERLAY_SELECTOR =
  "[role='menu'], [role='listbox'], [role='dialog'], [role='alertdialog']"

const PRESSED_BY_ENTER_SELECTOR =
  "button, a[href], summary, [role='button'], [role='tab'], [role='menuitem'], [role='option'], [role='switch'], [role='checkbox'], [role='radio'], [role='slider'], [role='separator']"

/**
 * Keys that run their action whatever control has the focus. Space plays
 * and stops, and it must keep doing that after a click on a button or a
 * fader has left the focus there.
 */
const TRANSPORT_CHORDS = new Set(["Space"])

export function isTransportChord(normalized: string): boolean {
  return TRANSPORT_CHORDS.has(normalized)
}

function splitChord(normalized: string) {
  const modifiers = normalized.split("+")
  const key = modifiers.pop() ?? ""
  return {
    key,
    isFunctionKey: /^F\d{1,2}$/.test(key),
    hasCommand: modifiers.includes("Ctrl") || modifiers.includes("Meta"),
    hasAlt: modifiers.includes("Alt"),
  }
}

/**
 * Whether a shortcut may fire while the user types in a field. Plain keys
 * must reach the field, and so must the editing shortcuts every text box
 * has, such as undo and paste.
 */
export function firesWhileTyping(normalized: string): boolean {
  const { key, isFunctionKey, hasCommand, hasAlt } = splitChord(normalized)
  if (isFunctionKey) return true
  if (!hasCommand) return false
  return !(TEXT_EDITING_KEYS.has(key) && !hasAlt)
}

/**
 * Whether a shortcut may fire while a menu, popover or dialog has the focus.
 * Those use plain keys to move around, so only shortcuts with Ctrl or Cmd
 * and the function keys get through.
 */
export function firesInOverlay(normalized: string): boolean {
  const { isFunctionKey, hasCommand } = splitChord(normalized)
  return isFunctionKey || hasCommand
}

export function isTypingTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element && target.closest(TEXT_ENTRY_SELECTOR) !== null
  )
}

/**
 * Whether the keymap may act on a key, given what has the focus. This is
 * the one rule for it; no panel decides this for itself.
 *
 * - While typing (an input, a text area, editable text, a combo box, the
 *   inline value entry of a knob), only function keys and the Ctrl or Cmd
 *   shortcuts that are not text editing get through.
 * - An open menu, list or dialog keeps every plain key, Space included.
 * - Anywhere else every shortcut is allowed. That includes Space on a
 *   focused button, slider, step or tab, which the keymap takes before the
 *   control sees it. Enter is the key that presses the focused control, so
 *   a shortcut on plain Enter does not fire there.
 */
export function shortcutAllowed(
  target: EventTarget | null,
  normalized: string
): boolean {
  if (!(target instanceof Element)) return true
  if (target.closest(TEXT_ENTRY_SELECTOR)) return firesWhileTyping(normalized)
  if (target.closest(OVERLAY_SELECTOR)) return firesInOverlay(normalized)
  if (normalized === "Enter") {
    return target.closest(PRESSED_BY_ENTER_SELECTOR) === null
  }
  return true
}
