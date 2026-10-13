import { Add01Icon, Cancel01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { useMemo } from "react"

import type { Send, TrackId } from "@/bindings"
import { faderTaper, gainUnit, Knob } from "@/components/audio"
import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ValueContextItems } from "@/components/value-context-menu"
import { automationFeed, useAutomationMarker } from "@/features/automation/live"
import { dispatch, useHint, useProjectStore } from "@/lib/store"
import { MAX_GAIN } from "@/lib/units"
import { cn } from "@/lib/utils"

import { trackValueItems } from "./menus"
import { addSend, clampGain, removeSend } from "./operations"
import { LoopNote, TrackLabel } from "./output-select"
import { sendChoices } from "./routing"
import { nextSendPreset } from "./send-preset-step"
import { nextSendGain, SEND_PRESETS } from "./send-presets"
import { nextSendGainScale } from "./send-scale"
import { useGestureValue } from "./use-gesture-value"
import { message, translatedText as t, dynamicTitle } from "@/lib/i18n"

function SendRow({ from, send }: { from: TrackId; send: Send }) {
  const target = useProjectStore(
    (state) =>
      state.project.mixer.tracks.find((track) => track.id === send.target)
        ?.name ?? "a removed track"
  )
  const level = useGestureValue(
    send.gain,
    (gain) => ({ type: "setSend", from, to: send.target, gain }),
    clampGain
  )
  const hint = useHint(
    message("How much of this track goes to \"{target}\", after its fader. Double-click for 0 dB", { target })
  )

  const items = useMemo(
    (): ContextItem[] => [
      ...trackValueItems(from, { send: send.target }),
      {
        submenu: t("Send level"),
        items: [
          ...SEND_PRESETS.map((preset) => ({
            title: preset.label,
            disabled: nextSendGain(send.gain, preset.value) === null,
            run: () => {
              const gain = nextSendGain(send.gain, preset.value)
              if (gain === null) return
              return dispatch({ type: "setSend", from, to: send.target, gain })
            },
          })),
          ...(["previous", "next"] as const).map((direction) => ({
            title: direction === "previous" ? t("Previous preset") : t("Next preset"),
            disabled: nextSendPreset(send.gain, direction) === null,
            run: () => {
              const latest = useProjectStore
                .getState()
                .project.mixer.tracks.find((track) => track.id === from)
                ?.sends.find((item) => item.target === send.target)
              if (!latest) return
              const next = nextSendPreset(latest.gain, direction)
              if (next === null) return
              return dispatch({
                type: "setSend",
                from,
                to: send.target,
                gain: next,
              })
            },
          })),
          ...(["half", "double"] as const).map((factor) => ({
            title: factor === "half" ? t("Half") : t("Double"),
            disabled: nextSendGainScale(send.gain, factor) === null,
            run: () => {
              const latest = useProjectStore
                .getState()
                .project.mixer.tracks.find((track) => track.id === from)
                ?.sends.find((item) => item.target === send.target)
              if (!latest) return
              const next = nextSendGainScale(latest.gain, factor)
              if (next === null) return
              return dispatch({
                type: "setSend",
                from,
                to: send.target,
                gain: next,
              })
            },
          })),
        ],
      },
      contextSeparator,
      {
        ...dynamicTitle("Remove send", {}),
        destructive: true,
        run: () => removeSend(from, send.target),
      },
    ],
    [from, send.target, send.gain]
  )
  const live = useMemo(
    () =>
      automationFeed({ type: "sendGain", track: from, target: send.target }),
    [from, send.target]
  )
  const marker = useAutomationMarker({
    type: "sendGain",
    track: from,
    target: send.target,
  })

  return (
    <li
      data-slot="track-send"
      className="group/send relative flex h-[26px] shrink-0 items-center gap-1"
    >
      <ValueContextItems items={items}>
        <Knob
          size="sm"
          min={0}
          max={MAX_GAIN}
          scale={faderTaper}
          defaultValue={1}
          aria-label={message("Send to {target}", { target })}
          live={live}
          marker={marker}
          {...gainUnit}
          {...level}
          {...hint}
        />
      </ValueContextItems>
      <span
        title={target}
        className="min-w-0 flex-1 truncate text-[10px] leading-none"
      >
        {target}
      </span>
      {/* A strip is too narrow for the name and the button side by side,
          so the button lies over the end of the name while the row is
          pointed at or focused. */}
      <button
        type="button"
        aria-label={message("Remove the send to {target}", { target })}
        className="absolute top-1/2 right-0 flex size-4 -translate-y-1/2 items-center justify-center rounded-sm bg-muted text-muted-foreground opacity-0 outline-none group-hover/send:opacity-100 hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring pointer-coarse:opacity-100"
        onClick={() => void removeSend(from, send.target)}
      >
        <HugeiconsIcon
          icon={Cancel01Icon}
          strokeWidth={2}
          className="size-2.5"
        />
      </button>
    </li>
  )
}

/** The sends of a track: a level knob, the target and a remove button each. */
export function SendList({
  id,
  sends,
  className,
}: {
  id: TrackId
  sends: Send[]
  className?: string
}) {
  if (sends.length === 0) return null
  return (
    <ul
      aria-label={t("Sends")}
      className={cn(
        "flex flex-col overflow-x-hidden overflow-y-auto",
        className
      )}
    >
      {sends.map((send) => (
        <SendRow key={send.target} from={id} send={send} />
      ))}
    </ul>
  )
}

// Mounted only while the menu is open, so it can follow every track.
function SendChoices({ id }: { id: TrackId }) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const { tracks: open, looping } = sendChoices(tracks, id)
  return (
    <>
      {open.length === 0 && (
        <DropdownMenuItem disabled>{t("No track left to send to")}</DropdownMenuItem>
      )}
      {open.map((track) => (
        <DropdownMenuItem
          key={track.id}
          onClick={() => void addSend(id, track.id)}
        >
          <TrackLabel track={track} number={tracks.indexOf(track)} />
        </DropdownMenuItem>
      ))}
      <LoopNote count={looping} />
    </>
  )
}

/** Opens the list of tracks this one can still send to. */
export function AddSendMenu({
  id,
  className,
}: {
  id: TrackId
  className?: string
}) {
  const hint = useHint(
    t("Send a copy of this track to another one, such as a shared reverb")
  )
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            data-slot="track-add-send"
            aria-label={t("Add send")}
            className={cn(
              "flex h-[18px] min-w-0 items-center gap-0.5 rounded-[3px] pr-1 pl-0.5 text-[10px] leading-none text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground",
              className
            )}
            {...hint}
          />
        }
      >
        <HugeiconsIcon
          icon={Add01Icon}
          strokeWidth={2}
          className="size-3 shrink-0"
        />
        <span className="truncate">{t("Send")}</span>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="max-h-80 w-auto min-w-44">
        <SendChoices id={id} />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
