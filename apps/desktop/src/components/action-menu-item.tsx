import { Tick02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useActionTitle, useTranslation } from "@/lib/i18n"

import {
  DropdownMenuItem,
  DropdownMenuShortcut,
} from "@/components/ui/dropdown-menu"
import {
  disabledReason,
  isChecked,
  isEnabled,
  runAction,
  shortcutLabel,
  type Action,
  type AppState,
} from "@/lib/actions"

type ActionMenuItemProps = {
  action: Action
  state: AppState
  /** Leaves room for a tick, to line up with on/off items in the same menu. */
  inset?: boolean
}

/** The tick of an on/off item, or the room it would take. */
export function MenuTick({ checked }: { checked: boolean }) {
  return (
    <span className="flex size-3.5 items-center justify-center" aria-hidden>
      {checked && <HugeiconsIcon icon={Tick02Icon} strokeWidth={2} />}
    </span>
  )
}

/**
 * One registry action as an item of a dropdown menu or the menu bar. Title,
 * shortcut, on/off tick and disabled state all come from the registry.
 */
export function ActionMenuItem({ action, state, inset }: ActionMenuItemProps) {
  const t = useTranslation()
  const title = useActionTitle(action.id, action.title)
  const shortcut = shortcutLabel(action.id)
  const checkable = action.checked !== undefined
  const checked = isChecked(action, state)
  const enabled = isEnabled(action, state)
  // A disabled item says why where its shortcut would be.
  const note = enabled ? shortcut : (disabledReason(action, state) ?? shortcut)

  return (
    <DropdownMenuItem
      role={checkable ? "menuitemcheckbox" : "menuitem"}
      aria-checked={checkable ? checked : undefined}
      disabled={!enabled}
      onClick={() => void runAction(action.id)}
    >
      {(checkable || inset) && <MenuTick checked={checked} />}
      <span className="whitespace-nowrap">{title}</span>
      {note && (
        <DropdownMenuShortcut
          className={enabled ? "pl-6" : "pl-6 tracking-normal"}
        >
          {enabled ? note : t(note)}
        </DropdownMenuShortcut>
      )}
    </DropdownMenuItem>
  )
}
