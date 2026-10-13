import { ValueContextItems } from "@/components/value-context-menu"
import { automationFeed, useAutomationMarker } from "@/features/automation/live"
import { trackValueItems } from "./menus"
import { useMemo } from "react"
import type { Send, TrackId } from "@/bindings"
import { faderTaper, gainUnit, Knob } from "@/components/audio"
import type { ContextItem } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { dispatch, useProjectStore } from "@/lib/store"
import { MAX_GAIN } from "@/lib/units"
import { clampGain, removeSidechain } from "./operations"
import { outputChoices } from "./routing"
import { nextSidechainPreset } from "./sidechain-preset-step"
import { nextSidechainGain, SIDECHAIN_PRESETS } from "./sidechain-presets"
import { nextSidechainGainScale } from "./sidechain-scale"
import { useGestureValue } from "./use-gesture-value"
import { translatedText as t, message } from "@/lib/i18n"

function SidechainRow({ from, send }: { from: TrackId; send: Send }) {
  const target = useProjectStore(
    (state) =>
      state.project.mixer.tracks.find((track) => track.id === send.target)
        ?.name ?? "Removed track"
  )
  const level = useGestureValue(
    send.gain,
    (gain) => ({ type: "setSidechain", from, to: send.target, gain }),
    clampGain
  )
  const items = useMemo(
    (): ContextItem[] => [
      ...trackValueItems(from, { sidechain: send.target }),
      {
        submenu: t("Sidechain level"),
        items: [
          ...SIDECHAIN_PRESETS.map((preset) => ({
            title: preset.label,
            disabled: nextSidechainGain(send.gain, preset.value) === null,
            run: () => {
              const gain = nextSidechainGain(send.gain, preset.value)
              if (gain === null) return
              return dispatch({
                type: "setSidechain",
                from,
                to: send.target,
                gain,
              })
            },
          })),
          ...(["previous", "next"] as const).map((direction) => ({
            title: direction === "previous" ? t("Previous preset") : t("Next preset"),
            disabled: nextSidechainPreset(send.gain, direction) === null,
            run: () => {
              const sidechain = useProjectStore
                .getState()
                .project.mixer.tracks.find((track) => track.id === from)
                ?.sidechains?.find((item) => item.target === send.target)
              if (!sidechain) return
              const next = nextSidechainPreset(sidechain.gain, direction)
              if (next === null) return
              return dispatch({
                type: "setSidechain",
                from,
                to: send.target,
                gain: next,
              })
            },
          })),
          ...(["half", "double"] as const).map((factor) => ({
            title: factor === "half" ? t("Half") : t("Double"),
            disabled: nextSidechainGainScale(send.gain, factor) === null,
            run: () => {
              const sidechain = useProjectStore
                .getState()
                .project.mixer.tracks.find((track) => track.id === from)
                ?.sidechains?.find((item) => item.target === send.target)
              if (!sidechain) return
              const next = nextSidechainGainScale(sidechain.gain, factor)
              if (next === null) return
              return dispatch({
                type: "setSidechain",
                from,
                to: send.target,
                gain: next,
              })
            },
          })),
        ],
      },
    ],
    [from, send.target, send.gain]
  )
  const targetKey = useMemo(
    () =>
      ({ type: "sidechainGain", track: from, target: send.target }) as const,
    [from, send.target]
  )
  const live = useMemo(() => automationFeed(targetKey), [targetKey])
  const marker = useAutomationMarker(targetKey)
  return (
    <li className="flex items-center gap-2">
      <ValueContextItems items={items}>
        <Knob
          live={live}
          marker={marker}
          size="sm"
          min={0}
          max={MAX_GAIN}
          scale={faderTaper}
          defaultValue={1}
          aria-label={message("Sidechain level to {target}", { target })}
          {...gainUnit}
          {...level}
        />
      </ValueContextItems>
      <span className="min-w-0 flex-1 truncate text-xs">{target}</span>
      <Button
        size="sm"
        variant="ghost"
        aria-label={message("Remove sidechain to {target}", { target })}
        onClick={() => void removeSidechain(from, send.target)}
      >
        ×
      </Button>
    </li>
  )
}

export function SidechainPanel({ track }: { track: TrackId }) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const source = tracks.find((item) => item.id === track)
  const sends = useMemo(() => source?.sidechains ?? [], [source?.sidechains])
  const incoming = tracks.filter((item) =>
    item.sidechains?.some((send) => send.target === track)
  )
  const choices = useMemo(
    () =>
      outputChoices(tracks, track).tracks.filter(
        (item) => !sends.some((send) => send.target === item.id)
      ),
    [tracks, track, sends]
  )
  if (!source || source.current) return null
  return (
    <div className="shrink-0 border-b p-1.5">
      <Popover>
        <PopoverTrigger
          render={
            <Button
              variant="outline"
              size="sm"
              className="w-full justify-start"
            >
              {t("Sidechain ·")}{" "}{incoming.length}{" "}{t("in /")}{" "}{sends.length}{" "}{t("out")}
            </Button>
          }
        />
        <PopoverContent align="start" className="w-80 space-y-3">
          <p className="text-xs text-muted-foreground">
            {t("Detector-only audio is kept separate from the destination's audible input. Enable External sidechain on its compressor to use the combined key bus.")}
          </p>
          <p className="text-xs font-medium">{t("Detector inputs")}</p>
          {incoming.length ? (
            <ul className="max-h-36 space-y-1 overflow-y-auto">
              {incoming.map((item) => (
                <li key={item.id} className="flex items-center gap-1 text-xs">
                  <span className="min-w-0 flex-1 truncate">{item.name}</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={message("Remove detector input from {itemName}", { itemName: item.name })}
                    onClick={() => void removeSidechain(item.id, track)}
                  >
                    ×
                  </Button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-xs text-muted-foreground">
              {t("No external detector signal.")}
            </p>
          )}
          {track !== 0 && (
            <>
              <p className="text-xs font-medium">{t("Send to detectors")}</p>
              <ul className="max-h-48 space-y-1 overflow-y-auto">
                {sends.map((send) => (
                  <SidechainRow key={send.target} from={track} send={send} />
                ))}
              </ul>
              <Select
                value={null}
                disabled={!choices.length}
                items={choices.map((item) => ({
                  value: item.id,
                  label: item.name,
                }))}
                onValueChange={(to: number | null) => {
                  if (to !== null)
                    void dispatch({
                      type: "setSidechain",
                      from: track,
                      to,
                      gain: 1,
                    })
                }}
              >
                <SelectTrigger aria-label={t("Add detector-only sidechain")}>
                  <SelectValue placeholder={t("Add sidechain destination…")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {choices.map((item) => (
                      <SelectItem key={item.id} value={item.id}>
                        {item.name}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </>
          )}
        </PopoverContent>
      </Popover>
    </div>
  )
}
