import type { ComponentProps, ReactNode } from "react"

import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { useHint } from "@/lib/store/hint"
import { message, useActionTitle, useTranslation } from "@/lib/i18n"
import {
  runAction,
  useAction,
  useActionEnabled,
  useShortcutLabel,
} from "@/lib/actions"

type ActionButtonProps = Omit<
  ComponentProps<typeof Button>,
  "onClick" | "children"
> & {
  /** Id of the registry action the button runs. */
  action: string
  /**
   * Usually an icon. Leave out to show the action's title as text. Text
   * given here is the button's name; an icon is named after the action.
   */
  children?: ReactNode
  tooltipSide?: "top" | "bottom" | "left" | "right"
}

/**
 * A button for a registry action. Its label, tooltip, shortcut hint and
 * disabled state all come from the registry, so a button and the menu item
 * for the same action never disagree.
 */
export function ActionButton({
  action: id,
  children,
  tooltipSide = "bottom",
  disabled,
  ...props
}: ActionButtonProps) {
  const action = useAction(id)
  const enabled = useActionEnabled(id)
  const shortcut = useShortcutLabel(id)
  const title = useActionTitle(id, action?.title ?? id).replace(/…$/, "")
  const t = useTranslation()
  const hint = useHint(shortcut ? message("{help} ({shortcut})", { help: title, shortcut }) : title)
  const showsText =
    children === undefined ||
    typeof children === "string" ||
    typeof children === "number"

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            aria-label={showsText ? undefined : title}
            disabled={disabled || !enabled}
            onClick={() => void runAction(id)}
            {...hint}
            {...props}
          />
        }
      >
        {typeof children === "string" ? t(children) : children ?? title}
      </TooltipTrigger>
      <TooltipContent side={tooltipSide}>
        {title}
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}
