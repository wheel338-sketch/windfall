import { memo } from "react"

import type { ChannelId } from "@/bindings"
import { ToggleLed } from "@/components/audio"
import { useHint } from "@/lib/store/hint"
import { cn } from "@/lib/utils"

import { toggleMute, toggleSolo } from "./channel-ops"
import { message, translatedText as t } from "@/lib/i18n"

type MuteLampProps = {
  id: ChannelId
  name: string
  muted: boolean
  solo: boolean
  /** Another channel is soloed, so this one is silent although it is on. */
  silenced: boolean
}

/**
 * The lamp at the start of a row. Lit means the channel plays. Click turns
 * it off and on; right-click or Ctrl-click solos, which turns the lamp
 * yellow with an S.
 */
export const MuteLamp = memo(function MuteLamp({
  id,
  name,
  muted,
  solo,
  silenced,
}: MuteLampProps) {
  const hint = useHint(
    solo
      ? message("{name} is soloed. Right-click or Ctrl-click to hear every channel again", { name })
      : message("{name} is {muted}. Click to {muted2}, right-click or Ctrl-click to solo", { name, muted: muted ? t("muted") : t("on"), muted2: muted ? t("unmute") : t("mute") })
  )

  return (
    <ToggleLed
      variant="dot"
      size="lg"
      pressed={!muted}
      color={solo ? "var(--wf-meter-mid)" : "var(--wf-meter-low)"}
      aria-label={message("{name} on", { name })}
      data-solo={solo ? "" : undefined}
      className={cn(
        "text-[9px] leading-none text-[oklch(0.2_0_0)]",
        solo &&
          "ring-1 ring-(--wf-meter-mid) ring-offset-1 ring-offset-background",
        silenced && "opacity-35"
      )}
      onClick={(event) => {
        if (event.ctrlKey || event.metaKey) {
          event.preventDefault()
          void toggleSolo(id)
        }
      }}
      onPressedChange={() => void toggleMute(id)}
      onContextMenu={(event) => {
        event.preventDefault()
        void toggleSolo(id)
      }}
      {...hint}
    >
      {solo && !muted ? "S" : null}
    </ToggleLed>
  )
})
