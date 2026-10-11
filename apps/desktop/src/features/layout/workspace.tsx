import { Cancel01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { ContextActions } from "@/components/context-actions"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { Kbd } from "@/components/ui/kbd"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { EnlargedEffects } from "@/features/mixer/inspector"
import { runAction, useShortcutLabel } from "@/lib/actions"
import { useChannelCount, useProjectReady } from "@/lib/store/selectors"
import { useUiStore, type CenterTab, type PanelSizes } from "@/lib/store/ui"
import { cn } from "@/lib/utils"
import { message, useTranslation } from "@/lib/i18n"

import { dividerMenu, EMPTY_PROJECT_MENU, TABS_MENU } from "./chrome-menus"
import { EmptyProject } from "./empty-states"
import { PanelBoundary, PanelFrame, PanelWindowButton } from "./panel-frame"
import { CENTER_TABS, PANELS } from "./panels"

/**
 * Sizes are saved per group and per set of visible panels, so the workspace
 * with the mixer hidden remembers a different split than with it showing.
 */
function useSavedLayout(group: string, panels: string[]) {
  const key = `${group}:${panels.join("+")}`
  const saved = useUiStore((state) => state.layouts[key])
  const saveLayout = useUiStore((state) => state.saveLayout)
  return {
    defaultLayout: saved,
    onLayoutChanged: (sizes: PanelSizes) => saveLayout(key, sizes),
  }
}

const MIXER_DIVIDER_MENU = dividerMenu("main", "view.mixer")
const BROWSER_DIVIDER_MENU = dividerMenu("workspace", "view.browser")

function CenterTabButton({ tab, active }: { tab: CenterTab; active: boolean }) {
  const t = useTranslation()
  const { title, action } = PANELS[tab]
  const shortcut = useShortcutLabel(action)

  return (
    <Tooltip>
      <TooltipTrigger
        role="tab"
        id={`center-tab-${tab}`}
        aria-selected={active}
        aria-controls="center-panel"
        tabIndex={active ? 0 : -1}
        onClick={() => void runAction(action)}
        className={cn(
          "relative flex h-7 items-center px-2.5 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:bg-accent focus-visible:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
          active &&
            "font-medium text-foreground after:absolute after:inset-x-2.5 after:bottom-0 after:h-0.5 after:rounded-t-full after:bg-brand"
        )}
      >
        {t(title)}
      </TooltipTrigger>
      <TooltipContent side="bottom">
        {message("Show the {panel}", { panel: t(title).toLowerCase() })}
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * The tab of the effects while they are enlarged into the editor area. It
 * is there only for as long as they are, at the end of the row, and a click
 * on it sends them back beside the mixer's strips.
 */
function EffectsTab() {
  const shortcut = useShortcutLabel("mixer.effectsBack")
  return (
    <Tooltip>
      <TooltipTrigger
        role="tab"
        id="center-tab-effects"
        aria-selected
        aria-controls="center-panel"
        tabIndex={0}
        onClick={() => void runAction("mixer.enlargeEffects")}
        className="relative flex h-7 items-center gap-1.5 px-2.5 text-xs font-medium text-foreground outline-none after:absolute after:inset-x-2.5 after:bottom-0 after:h-0.5 after:rounded-t-full after:bg-brand focus-visible:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        Effects
        <HugeiconsIcon
          icon={Cancel01Icon}
          strokeWidth={2}
          className="size-3 text-muted-foreground"
        />
      </TooltipTrigger>
      <TooltipContent side="bottom">
        Return the effects to the mixer
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}

/** The middle of the window: channel rack, playlist and piano roll as tabs. */
function CenterDock() {
  const preferredTab = useUiStore((state) => state.centerTab)
  const detached = useUiStore((state) => state.detachedPanels)
  const tabs = CENTER_TABS.filter((id) => !detached.includes(id))
  const tab = tabs.includes(preferredTab) ? preferredTab : tabs[0]
  const overlay = useUiStore((state) => state.centerOverlay)
  const ready = useProjectReady()
  const channelCount = useChannelCount()
  const info = tab ? PANELS[tab] : null
  const Panel = info?.component
  const showStart = tab === "channelRack" && ready && channelCount === 0

  function onKeyDown(event: React.KeyboardEvent) {
    const step =
      event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0
    if (step === 0 || !tab) return
    event.preventDefault()
    const next = tabs[(tabs.indexOf(tab) + step + tabs.length) % tabs.length]
    void runAction(PANELS[next].action)
    document.getElementById(`center-tab-${next}`)?.focus()
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-background">
      <ContextActions items={TABS_MENU}>
        <div
          role="tablist"
          aria-label="Editors"
          onKeyDown={onKeyDown}
          className="flex h-7 shrink-0 items-center border-b bg-chassis/60 px-1"
        >
          {tabs.map((item) => (
            <CenterTabButton
              key={item}
              tab={item}
              active={item === tab && overlay === null}
            />
          ))}
          {overlay === "effects" && <EffectsTab />}
          {tab && info && overlay === null && (
            <div className="ml-auto pr-1">
              <PanelWindowButton panel={tab} title={info.title} />
            </div>
          )}
        </div>
      </ContextActions>
      <div
        role="tabpanel"
        id="center-panel"
        aria-labelledby={
          overlay || tab ? `center-tab-${overlay ?? tab}` : undefined
        }
        aria-label={!overlay && !tab ? "Editors" : undefined}
        className="min-h-0 flex-1 overflow-auto"
      >
        {overlay === "effects" ? (
          <PanelBoundary key="effects" name="effects">
            <EnlargedEffects />
          </PanelBoundary>
        ) : info && Panel ? (
          <PanelBoundary key={tab} name={info.title.toLowerCase()}>
            {showStart ? (
              <ContextActions items={EMPTY_PROJECT_MENU}>
                <div className="h-full">
                  <EmptyProject />
                </div>
              </ContextActions>
            ) : (
              <Panel />
            )}
          </PanelBoundary>
        ) : (
          <p className="p-4 text-sm text-muted-foreground">
            The panels are in their own windows.
          </p>
        )}
      </div>
    </div>
  )
}

function SideDock({ panel }: { panel: "browser" | "mixer" }) {
  const { title, action, component: Panel } = PANELS[panel]
  return (
    <PanelFrame title={title} panel={panel} hideAction={action}>
      <Panel />
    </PanelFrame>
  )
}

function MainColumn() {
  const mixer = useUiStore(
    (state) => state.panels.mixer && !state.detachedPanels.includes("mixer")
  )
  const layout = useSavedLayout(
    "main",
    mixer ? ["center", "mixer"] : ["center"]
  )

  return (
    <ResizablePanelGroup id="main" orientation="vertical" {...layout}>
      <ResizablePanel id="center" minSize={140}>
        <CenterDock />
      </ResizablePanel>
      {mixer && (
        <>
          <ContextActions items={MIXER_DIVIDER_MENU}>
            <ResizableHandle aria-label="Resize the mixer" />
          </ContextActions>
          <ResizablePanel
            id="mixer"
            defaultSize="38%"
            minSize={120}
            maxSize="75%"
          >
            <SideDock panel="mixer" />
          </ResizablePanel>
        </>
      )}
    </ResizablePanelGroup>
  )
}

/**
 * The docked panels between the transport and the status bar: the browser on
 * the left, the editors in the middle and the mixer below them. Each one can
 * be resized and hidden, and the arrangement is remembered.
 */
export function Workspace() {
  const browser = useUiStore(
    (state) => state.panels.browser && !state.detachedPanels.includes("browser")
  )
  const generation = useUiStore((state) => state.layoutGeneration)
  const layout = useSavedLayout(
    "workspace",
    browser ? ["browser", "main"] : ["main"]
  )

  return (
    <ResizablePanelGroup
      // "Reset layout" bumps the generation, which rebuilds the groups from
      // their default sizes.
      key={generation}
      id="workspace"
      orientation="horizontal"
      className="min-h-0 flex-1"
      {...layout}
    >
      {browser && (
        <>
          <ResizablePanel
            id="browser"
            defaultSize="19%"
            minSize={180}
            maxSize="45%"
          >
            <SideDock panel="browser" />
          </ResizablePanel>
          <ContextActions items={BROWSER_DIVIDER_MENU}>
            <ResizableHandle aria-label="Resize the browser" />
          </ContextActions>
        </>
      )}
      <ResizablePanel id="main" minSize={360}>
        <MainColumn />
      </ResizablePanel>
    </ResizablePanelGroup>
  )
}
