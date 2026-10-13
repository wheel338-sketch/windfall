import {
  cloneElement,
  createContext,
  useContext,
  useEffect,
  useRef,
  type MouseEvent,
  type ReactNode,
} from "react"
import { create } from "zustand"

import {
  PLAIN_NUMBER,
  ValueControlSlot,
  type ValueControlActions,
  type ValueControlSlotProps,
} from "@/components/audio"

import { isTextField } from "@/lib/text-field-menu"
import { dynamicTitle, message } from "@/lib/i18n"

import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "./context-actions"

/*
 * The right-click menu of every value control: knobs, faders, pan controls
 * and number fields, and through them every setting of an effect or an
 * instrument. The kit knows nothing of menus. It hands each control to a
 * slot, and this file fills the slot for the whole app.
 *
 * There is one menu for all of them, opened where the pointer is. A menu
 * of its own for each control would be hundreds of menus that render again
 * with every step of a drag.
 */

const NO_ITEMS: readonly ContextItem[] = []

/** A value copied from a control. */
export type CopiedValue = {
  /** The number itself, in the unit of the control it came from. */
  value: number
  /** What that number is, when the control said: "gain", "pan", "hertz". */
  unitKind: string | undefined
  /** Its readout: "−18.0 dB". */
  text: string
  /** Numeric input for a bare-number readout, independent of its locale. */
  canonicalText?: string
}

/**
 * The value last copied from a control. It is kept here as a number with
 * the kind of unit it is in, so a level is never pasted into a pan as
 * whatever its digits happen to mean there. The readout goes to the system
 * clipboard as well, for pasting into a text field or another program;
 * reading that back would need a permission the webview asks for every
 * time.
 */
const useCopiedValue = create<{ copied: CopiedValue | null }>(() => ({
  copied: null,
}))

function copyValue(copied: CopiedValue) {
  useCopiedValue.setState({ copied })
  // Best effort: the value is also there to paste into a text field.
  void navigator.clipboard?.writeText(copied.text).catch(() => undefined)
}

/** Forgets the copied value. For tests. */
export function clearCopiedValue() {
  useCopiedValue.setState({ copied: null })
}

/** What pasting the copied value into a control would do. */
export type PasteOutcome =
  /** The value the control would get. */
  | { value: number }
  /** Why it cannot be pasted, in a few words. */
  | { reason: string }

/**
 * Whether a copied value can go into a control, and as what.
 *
 * - The same kind of unit: the number as it is, held to the control's range.
 * - A bare number: read the way typing it into the control would be.
 * - Two controls that both say nothing about their unit: by the readout,
 *   as typing it would be, which is all there is to go by.
 * - Anything else is another unit, and is not pasted.
 */
export function pasteOutcome(
  control: Pick<ValueMenuTarget, "unitKind" | "parse">,
  copied: CopiedValue
): PasteOutcome {
  const sameKind =
    copied.unitKind !== undefined && copied.unitKind === control.unitKind
  if (sameKind && copied.unitKind !== PLAIN_NUMBER) {
    return { value: copied.value }
  }
  const byText =
    sameKind ||
    copied.unitKind === PLAIN_NUMBER ||
    (copied.unitKind === undefined && control.unitKind === undefined)
  if (!byText) return { reason: message("Another unit", {}, "en") }
  const parsed = control.parse(copied.canonicalText ?? copied.text)
  return parsed === null
    ? { reason: message("Not a value for this", {}, "en") }
    : { value: parsed }
}

/** What a value control's menu acts on. A kit control is one already. */
export type ValueMenuTarget = Pick<
  ValueControlActions,
  | "value"
  | "text"
  | "defaultValue"
  | "disabled"
  | "editable"
  | "unitKind"
  | "reset"
  | "startEditing"
  | "change"
  | "parse"
>

/**
 * The entries every value control has: back to the default, type a value,
 * copy and paste. `name` heads the menu with what was clicked, and `extra`
 * are the entries of whatever the control is bound to, put first.
 */
export function valueMenuItems(
  control: ValueMenuTarget,
  name: string | null,
  extra: readonly ContextItem[] = NO_ITEMS
): ContextItem[] {
  const { copied } = useCopiedValue.getState()
  const paste = copied === null ? null : pasteOutcome(control, copied)
  const { defaultValue } = control
  return [
    { label: name ? message("{name}: {value}", { name, value: control.text }) : control.text },
    ...extra,
    ...(extra.length > 0 ? [contextSeparator] : []),
    {
      ...dynamicTitle("Reset to default", {}),
      disabled:
        control.disabled ||
        defaultValue === undefined ||
        defaultValue === control.value,
      // Not Delete: on a control that key belongs to the panel around it.
      shortcut: "Ctrl+click",
      run: control.reset,
    },
    {
      ...dynamicTitle("Type in value…", {}),
      disabled: control.disabled || !control.editable,
      shortcut: "Enter",
      // The text field takes the focus, which the closing menu would take
      // back if the field opened first.
      afterClose: true,
      run: control.startEditing,
    },
    contextSeparator,
    {
      ...dynamicTitle("Copy value", {}),
      run: () =>
        copyValue({
          value: control.value,
          unitKind: control.unitKind,
          text: control.text,
          ...(control.unitKind === PLAIN_NUMBER ? { canonicalText: String(control.value) } : {}),
        }),
    },
    {
      ...(copied === null ? dynamicTitle("Paste value", {}) : dynamicTitle("Paste value ({value})", { value: copied.text })),
      disabled: control.disabled || paste === null || "reason" in paste,
      // A value in another unit says so, and stays out.
      reason: paste !== null && "reason" in paste ? paste.reason : undefined,
      run: () => {
        if (paste !== null && "value" in paste) control.change(paste.value)
      },
    },
  ]
}

type OpenMenu = {
  control: ValueMenuTarget
  name: string | null
  extra: readonly ContextItem[]
}

type SharedMenu = {
  /** The control the menu was last opened on. */
  opened: OpenMenu | null
  /** Opens the menu at a point of the window. Set by the host. */
  openAt: ((x: number, y: number) => void) | null
}

const useSharedMenu = create<SharedMenu>(() => ({ opened: null, openAt: null }))

const ExtraItems = createContext<readonly ContextItem[]>(NO_ITEMS)

type ValueContextItemsProps = {
  /** Entries for the menus of the value controls inside, put first. */
  items: readonly ContextItem[]
  children: ReactNode
}

/**
 * Adds entries to the right-click menu of the value controls inside it.
 * This is where a control that is bound to something in the project says
 * what else can be done with that: wrap the control, or the group of
 * controls, and give the entries.
 *
 *     <ValueContextItems items={["mixer.resetPeaks"]}>
 *       <Fader … />
 *     </ValueContextItems>
 */
export function ValueContextItems({ items, children }: ValueContextItemsProps) {
  return <ExtraItems value={items}>{children}</ExtraItems>
}

/** The name a control goes by: its label, as a screen reader has it. */
function nameOf(root: Element): string | null {
  const slider = root.matches("[role=slider]")
    ? root
    : root.querySelector("[role=slider]")
  if (!slider) return null
  const label = slider.getAttribute("aria-label")
  if (label) return label
  const labelledBy = slider.getAttribute("aria-labelledby")
  const text = labelledBy
    ? document.getElementById(labelledBy)?.textContent
    : null
  return text?.trim() || null
}

/** What the kit's slot is filled with: a right-click opens the one menu. */
function ValueMenuSlot({ control, children }: ValueControlSlotProps) {
  const extra = useContext(ExtraItems)
  const own = children.props.onContextMenu
  return cloneElement(children, {
    onContextMenu(event: MouseEvent<HTMLDivElement>) {
      own?.(event)
      // The text entry of a control keeps the menu every text field has.
      const onText = isTextField(event.target)
      const { openAt } = useSharedMenu.getState()
      if (event.defaultPrevented || onText || !openAt) return
      event.preventDefault()
      // The panel around the control has a menu too, which is not meant.
      event.stopPropagation()
      useSharedMenu.setState({
        opened: { control, name: nameOf(event.currentTarget), extra },
      })
      openAt(event.clientX, event.clientY)
    },
  })
}

/**
 * The one menu, and the element it is opened through. The element is never
 * seen or clicked: a right-click on a control is passed on to it with the
 * pointer's place, and the menu opens there.
 */
function ValueMenuHost() {
  const trigger = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    useSharedMenu.setState({
      openAt: (clientX, clientY) =>
        trigger.current?.dispatchEvent(
          new window.MouseEvent("contextmenu", {
            bubbles: true,
            cancelable: true,
            clientX,
            clientY,
          })
        ),
    })
    return () => useSharedMenu.setState({ openAt: null, opened: null })
  }, [])

  return (
    <ContextActions
      items={() => {
        const { opened } = useSharedMenu.getState()
        return opened
          ? valueMenuItems(opened.control, opened.name, opened.extra)
          : []
      }}
    >
      <span ref={trigger} data-slot="value-menu-host" hidden />
    </ContextActions>
  )
}

/**
 * Gives every value control inside it the app's right-click menu. Put it
 * once around the window.
 */
export function ValueContextMenus({ children }: { children: ReactNode }) {
  return (
    <ValueControlSlot value={ValueMenuSlot}>
      {children}
      <ValueMenuHost />
    </ValueControlSlot>
  )
}
