import { useRef, type ReactElement } from "react"

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuGroup,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import {
  disabledReason,
  isChecked,
  isEnabled,
  runAction,
  shortcutLabel,
  useActions,
  useAppState,
} from "@/lib/actions"
import { reportError } from "@/lib/errors"
import { isTextField } from "@/lib/text-field-menu"
import { actionTitle, message, useTranslation } from "@/lib/i18n"
import type { DynamicActionMessage } from "@/lib/i18n/action-messages"

import { MenuTick } from "./action-menu-item"

/** An entry that is not in the registry because it acts on the thing clicked. */
export type InlineAction = {
  title: string
  /** Retained dynamic titles render from their key in the current language. */
  titleMessage?: DynamicActionMessage
  run(): void | Promise<unknown>
  disabled?: boolean
  /**
   * A few words on why the entry is disabled, shown where its shortcut
   * would be, as a registry action's `whyDisabled` is.
   */
  reason?: string
  destructive?: boolean
  /** For an entry that is on or off. Shows a tick while it is on. */
  checked?: boolean
  /** Shown as a hint only. Bind real keys through the registry. */
  shortcut?: string
  /**
   * Run once the menu has closed and handed the focus back. For an entry
   * that puts the focus somewhere itself, such as one that opens a text
   * field: run at once, the closing menu would take the focus away again.
   */
  afterClose?: boolean
}

export type ContextItem =
  /** A registry action id. */
  | string
  | InlineAction
  | { separator: true }
  | { label: string; labelMessage?: DynamicActionMessage }
  /** A submenu with entries of its own. */
  | { submenu: string; submenuMessage?: DynamicActionMessage; items: ContextItem[] }
  /**
   * Entries worked out when the menu opens, for a list that is handed over
   * once and has to follow the project: what there is to show for the
   * thing clicked may have changed since.
   */
  | { dynamic: () => ContextItem[] }

export const contextSeparator = { separator: true } as const

type ContextActionsProps = {
  /**
   * What the menu offers, top to bottom. A function is asked each time the
   * menu opens, for entries that depend on what was clicked.
   */
  items: ContextItem[] | (() => ContextItem[])
  /** The element that opens the menu on right-click. Must take a ref. */
  children: ReactElement
}

/**
 * Gives an element a right-click menu. Entries are registry action ids or
 * inline actions for the thing that was clicked:
 *
 *     <ContextActions
 *       items={[
 *         { title: "Rename", run: rename },
 *         contextSeparator,
 *         "channel.add",
 *       ]}
 *     >
 *       <div>…</div>
 *     </ContextActions>
 *
 * Menus nest: a right-click opens the menu of the innermost element that
 * has one, so a panel can have a menu for its empty space and each thing
 * in it a menu of its own.
 */
export function ContextActions({ items, children }: ContextActionsProps) {
  // The entry that waits for the menu to finish closing, if one was picked.
  const pending = useRef<(() => void) | null>(null)
  // Such an entry puts the focus where it wants it, so the closing menu
  // must not hand the focus back to where it was. Until it opens again.
  const keepsFocus = useRef(false)
  return (
    <ContextMenu
      onOpenChange={(open) => {
        if (open) keepsFocus.current = false
      }}
      onOpenChangeComplete={(open) => {
        if (open) return
        const run = pending.current
        pending.current = null
        run?.()
      }}
    >
      <ContextMenuTrigger
        render={children}
        // A right-click that something inside has used for itself, such as
        // a step that is erased with it, is not also a menu out here. And a
        // text field keeps the menu every text field has, for its cut,
        // copy and paste.
        onContextMenu={(event) => {
          if (event.defaultPrevented || isTextField(event.target)) {
            event.preventBaseUIHandler()
          }
        }}
      />
      <ContextMenuContent
        className="min-w-44"
        finalFocus={() => !keepsFocus.current}
      >
        <ContextItems
          items={items}
          defer={(run) => {
            pending.current = run
            keepsFocus.current = true
          }}
        />
      </ContextMenuContent>
    </ContextMenu>
  )
}

function runInline(item: InlineAction) {
  new Promise((resolve) => resolve(item.run())).catch((error: unknown) =>
    reportError(error, item.titleMessage ?? item.title)
  )
}

/** The entries to show: every `dynamic` one replaced by what it gives now. */
function expanded(items: readonly ContextItem[]): ContextItem[] {
  return items.flatMap((item) =>
    typeof item === "object" && "dynamic" in item
      ? expanded(item.dynamic())
      : [item]
  )
}

type ContextItemsProps = {
  items: ContextItem[] | (() => ContextItem[])
  /** Keeps an entry to run once the menu has closed. */
  defer(run: () => void): void
}

// Rendered only while the menu is open, so reading the whole app state here
// costs nothing the rest of the time.
function ContextItems({ items: given, defer }: ContextItemsProps) {
  const t = useTranslation()
  const actions = useActions()
  const state = useAppState()
  const items = expanded(typeof given === "function" ? given() : given)
  const find = (id: string) => actions.find((action) => action.id === id)
  // With an on/off entry in the menu, every entry leaves room for the tick.
  const hasToggles = items.some((item) =>
    typeof item === "string"
      ? find(item)?.checked !== undefined
      : "checked" in item && item.checked !== undefined
  )

  return items.map((item, index) => {
    if (typeof item === "string") {
      const action = find(item)
      if (!action) return null
      const shortcut = shortcutLabel(item)
      const checkable = action.checked !== undefined
      const checked = isChecked(action, state)
      const enabled = isEnabled(action, state)
      // A disabled entry says why where its shortcut would be.
      const reason = disabledReason(action, state)
      return (
        <ContextMenuItem
          key={item}
          role={checkable ? "menuitemcheckbox" : "menuitem"}
          aria-checked={checkable ? checked : undefined}
          disabled={!enabled}
          onClick={() => void runAction(item)}
        >
          {hasToggles && <MenuTick checked={checked} />}
          {actionTitle(action.id, action.title)}
          {reason ? (
            <ContextMenuShortcut className="tracking-normal">
              {t(reason)}
            </ContextMenuShortcut>
          ) : (
            shortcut && <ContextMenuShortcut>{shortcut}</ContextMenuShortcut>
          )}
        </ContextMenuItem>
      )
    }
    if ("separator" in item) return <ContextMenuSeparator key={index} />
    if ("dynamic" in item) return null
    if ("submenu" in item) {
      return (
        <ContextMenuSub key={index}>
          <ContextMenuSubTrigger>
            {hasToggles && <MenuTick checked={false} />}
            {item.submenuMessage ? message(item.submenuMessage.key, item.submenuMessage.values) : item.submenu}
          </ContextMenuSubTrigger>
          <ContextMenuSubContent className="min-w-40">
            <ContextItems items={item.items} defer={defer} />
          </ContextMenuSubContent>
        </ContextMenuSub>
      )
    }
    if ("label" in item) {
      return (
        <ContextMenuGroup key={index}>
          <ContextMenuLabel>{item.labelMessage ? message(item.labelMessage.key, item.labelMessage.values) : item.label}</ContextMenuLabel>
        </ContextMenuGroup>
      )
    }
    const checkable = item.checked !== undefined
    return (
      <ContextMenuItem
        key={index}
        role={checkable ? "menuitemcheckbox" : "menuitem"}
        aria-checked={item.checked}
        disabled={item.disabled}
        variant={item.destructive ? "destructive" : "default"}
        onClick={() => {
          if (item.afterClose) defer(() => runInline(item))
          else runInline(item)
        }}
      >
        {hasToggles && <MenuTick checked={item.checked ?? false} />}
        {item.titleMessage ? message(item.titleMessage.key, item.titleMessage.values) : item.title}
        {item.disabled && item.reason ? (
          <ContextMenuShortcut className="tracking-normal">
            {t(item.reason)}
          </ContextMenuShortcut>
        ) : (
          item.shortcut && (
            <ContextMenuShortcut>{item.shortcut}</ContextMenuShortcut>
          )
        )}
      </ContextMenuItem>
    )
  })
}
