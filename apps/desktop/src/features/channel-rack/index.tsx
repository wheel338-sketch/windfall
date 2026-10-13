import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { useShortcutScope } from "@/lib/actions"
import { useUiStore, type PanelSizes } from "@/lib/store/ui"

import { ChannelInspector } from "./inspector"
import { LEFT_WIDTH, MIN_STEP_PITCH } from "./layout"
import { RackGrid } from "./rack-grid"
import { useRackStore } from "./rack-store"
import { RackToolbar } from "./rack-toolbar"
import { AdvancedFillDialog } from "./advanced-fill-dialog"
import { ChannelGroupsDialog } from "./channel-groups-ui"
import { translatedText as t } from "@/lib/i18n"

const LAYOUT_KEY = "channelRack:rows+settings"

/**
 * The channel rack: one row of steps per channel for the pattern being
 * edited, with the selected channel's settings docked on the right.
 */
export default function ChannelRackPanel() {
  const settingsOpen = useRackStore((state) => state.inspectorOpen)
  const savedLayout = useUiStore((state) => state.layouts[LAYOUT_KEY])
  const saveLayout = useUiStore((state) => state.saveLayout)
  const scope = useShortcutScope("channelRack")

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col" {...scope}>
      <RackToolbar />
      <AdvancedFillDialog />
      <ChannelGroupsDialog />
      <ResizablePanelGroup
        id="channel-rack"
        orientation="horizontal"
        className="min-h-0 flex-1"
        defaultLayout={settingsOpen ? savedLayout : undefined}
        onLayoutChanged={(sizes: PanelSizes) => {
          if (settingsOpen) saveLayout(LAYOUT_KEY, sizes)
        }}
      >
        <ResizablePanel
          id="rows"
          minSize={LEFT_WIDTH + 4 * MIN_STEP_PITCH}
          className="flex min-w-0 flex-col"
        >
          <RackGrid />
        </ResizablePanel>
        {settingsOpen && (
          <>
            <ResizableHandle aria-label={t("Resize the channel settings")} />
            <ResizablePanel
              id="settings"
              defaultSize={304}
              minSize={256}
              maxSize="55%"
              groupResizeBehavior="preserve-pixel-size"
            >
              <ChannelInspector />
            </ResizablePanel>
          </>
        )}
      </ResizablePanelGroup>
    </div>
  )
}
