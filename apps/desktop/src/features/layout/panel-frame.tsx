import { Cancel01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { Component, type ErrorInfo, type ReactNode } from "react"

import { ContextActions } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { runAction, useShortcutLabel } from "@/lib/actions"
import { backend, errorMessage } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import type { PanelId } from "@/lib/store/ui"
import { cn } from "@/lib/utils"
import { message, useTranslation, translatedText as t } from "@/lib/i18n"
import { openContextHelp } from "@/features/help/getting-started-store"

import { panelMenu } from "./chrome-menus"
import { detachPanel } from "./detach-panel"

type BoundaryProps = { name: string; children: ReactNode }
type BoundaryState = { error: unknown }

/** Keeps a panel that throws from taking the rest of the window with it. */
export class PanelBoundary extends Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = { error: null }

  static getDerivedStateFromError(error: unknown): BoundaryState {
    return { error }
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error(`The ${this.props.name} panel crashed`, error, info)
  }

  render() {
    if (this.state.error === null) return this.props.children
    return (
      <div
        role="alert"
        className="flex h-full flex-col items-center justify-center gap-2 p-4 text-center"
      >
        <p className="font-medium">{t("The")}{" "}{this.props.name}{" "}{t("stopped working.")}</p>
        <p className="max-w-sm text-muted-foreground">
          {errorMessage(this.state.error)}
        </p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => this.setState({ error: null })}
        >
          {t("Try again")}
        </Button>
      </div>
    )
  }
}

function HideButton({ title, action }: { title: string; action: string }) {
  const t = useTranslation()
  const shortcut = useShortcutLabel(action)
  const label = message("Hide {panel}", { panel: t(title).toLowerCase() })
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={label}
            className="text-muted-foreground"
            onClick={() => void runAction(action)}
          />
        }
      >
        <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
      </TooltipTrigger>
      <TooltipContent side="bottom">
        {label}
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}

export function PanelWindowButton({
  panel,
  title,
  dock = false,
}: {
  panel: PanelId
  title: string
  dock?: boolean
}) {
  const t = useTranslation()
  const label = message(dock ? "Dock {panel}" : "Detach {panel}", { panel: t(title).toLowerCase() })
  return (
    <Button
      variant="ghost"
      size="xs"
      aria-label={label}
      onClick={() => {
        const request = dock ? backend.dockPanel(panel) : detachPanel(panel)
        void request.catch((error: unknown) => reportError(error, label))
      }}
    >
      {t(dock ? "Dock" : "Detach")}
    </Button>
  )
}

type PanelFrameProps = {
  title: string
  panel?: PanelId
  detached?: boolean
  /** Registry action that shows and hides this panel. Adds a close button. */
  hideAction?: string
  /** Extra controls for the header, left of the close button. */
  actions?: ReactNode
  className?: string
  children: ReactNode
}

/** The frame every docked panel sits in: a title strip above a scrolling body. */
export function PanelFrame({
  title,
  panel,
  detached = false,
  hideAction,
  actions,
  className,
  children,
}: PanelFrameProps) {
  const t = useTranslation()
  return (
    <section
      aria-label={t(title)}
      className="flex h-full min-h-0 min-w-0 flex-col bg-background"
    >
      <ContextActions items={panelMenu(hideAction)}>
        <header className="flex h-7 shrink-0 items-center gap-1 border-b bg-chassis/60 pr-1 pl-2.5">
          <h2 className="truncate text-xs font-medium">{t(title)}</h2>
          <div className="ml-auto flex items-center gap-0.5">
            {actions}
            {panel && <Button variant="ghost" size="xs" onClick={() => openContextHelp(panel)}>{t("Help")}</Button>}
            {panel && (
              <PanelWindowButton panel={panel} title={title} dock={detached} />
            )}
            {hideAction && <HideButton title={title} action={hideAction} />}
          </div>
        </header>
      </ContextActions>
      <div className={cn("min-h-0 flex-1 overflow-auto", className)}>
        <PanelBoundary name={title.toLowerCase()}>{children}</PanelBoundary>
      </div>
    </section>
  )
}
