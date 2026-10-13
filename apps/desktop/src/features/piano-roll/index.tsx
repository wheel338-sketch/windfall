import { useEffect } from "react"
import { MusicNote03Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { ActionButton } from "@/components/action-button"
import { ContextActions } from "@/components/context-actions"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
import { useProjectGeneration } from "@/lib/store/replaced"
import { useChannelIds, useSelectedPatternId } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"

import { NO_CHANNEL_MENU } from "./menu"
import { Workspace } from "./workspace"
import { translatedText as t } from "@/lib/i18n"

function NoChannels() {
  return (
    <ContextActions items={NO_CHANNEL_MENU}>
      <Empty className="h-full">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <HugeiconsIcon icon={MusicNote03Icon} strokeWidth={2} />
          </EmptyMedia>
          <EmptyTitle>{t("There is no channel to write notes for")}</EmptyTitle>
          <EmptyDescription>
            {t("The piano roll edits the notes one channel plays in the current pattern. Add a channel, or drag a sample in from the browser, and its notes open here.")}
          </EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <div className="flex flex-wrap justify-center gap-2">
            <ActionButton action="channel.add" />
            <ActionButton action="view.channelRack" variant="outline">
              {t("Go to the channel rack")}
            </ActionButton>
          </div>
        </EmptyContent>
      </Empty>
    </ContextActions>
  )
}

/**
 * The piano roll: the notes of the selected channel in the pattern being
 * edited, as a grid with pitch going up and time going right. It edits the
 * same notes as the channel rack's steps.
 */
export default function PianoRollPanel() {
  const channelIds = useChannelIds()
  const patternId = useSelectedPatternId()
  const selected = useUiStore((state) => state.selectedChannel)
  const selectChannel = useUiStore((state) => state.selectChannel)
  const generation = useProjectGeneration()
  const channelId =
    selected !== null && channelIds.includes(selected)
      ? selected
      : (channelIds[0] ?? null)

  // With nothing selected the first channel is edited, and the rest of the
  // app should agree on which channel that is.
  useEffect(() => {
    if (channelId !== null && channelId !== selected) selectChannel(channelId)
  }, [channelId, selected, selectChannel])

  if (channelId === null || patternId === null) return <NoChannels />
  // Another project starts the editor over: its scroll, zoom and selection
  // belonged to the notes of the one before.
  return (
    <Workspace key={generation} patternId={patternId} channelId={channelId} />
  )
}
