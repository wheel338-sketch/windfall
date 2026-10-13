import { ArrowRight02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { memo, useMemo, useRef } from "react"

import type { TrackId } from "@/bindings"
import { MuteSolo, PanControl, ToggleLed } from "@/components/audio"
import type { ContextItem } from "@/components/context-actions"
import { ValueContextItems } from "@/components/value-context-menu"
import { automationFeed, useAutomationMarker } from "@/features/automation/live"
import { runAction } from "@/lib/actions"
import {
  useEngineStore,
  useHint,
  useMixerTrackIndex,
  useProjectStore,
  useUiStore,
} from "@/lib/store"
import { MASTER_TRACK } from "@/lib/units"
import { cn } from "@/lib/utils"

import { ChannelChips } from "./channel-chips"
import { useEffectDrop } from "./effect-drag"
import { EffectBadge, EffectRack } from "./effect-rack"
import { SEND_ROW_HEIGHT, type StripMode } from "./layout"
import { LevelSection, type LevelLayout } from "./level-section"
import { trackValueItems } from "./menus"
import { nextMixerPanPreset } from "./mixer-pan-preset-step"
import { nextMixerPanScale } from "./mixer-pan-scale"
import { patchTrack } from "./operations"
import { OutputSelect } from "./output-select"
import { PAN_PRESETS, nextPan } from "./pan-presets"
import { heardTracks, type Audibility } from "./routing"
import { RoutingButton } from "./routing-popover"
import { AddSendMenu, SendList } from "./sends"
import { StripHeader } from "./strip-header"
import { useStripTrack, type StripTrack } from "./strip-track"
import { useTrackGroupGesture } from "./group-gesture"
import { useMixerUi, selectMixerTrack } from "./mixer-ui"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/** Fades the parts of a strip that solo on another track has silenced. */
const DIMMED =
  "transition-opacity group-data-[audible=silenced]/strip:opacity-35"

function PanKnob({
  track,
  showValue,
}: {
  track: StripTrack
  showValue: boolean
}) {
  const pan = useTrackGroupGesture(track.id, "pan")
  const hint = useHint(t("Pan. Drag, or double-click to center"))
  // What the knob is bound to adds its own entries to the knob's menu.
  const items = useMemo<ContextItem[]>(
    () => [
      ...trackValueItems(track.id, "pan"),
      {
        submenu: t("Pan"),
        items: [
          ...PAN_PRESETS.map((preset) => ({
            title: preset.label,
            disabled: nextPan(track.pan, preset.value) === null,
            run: () => {
              const pan = nextPan(track.pan, preset.value)
              if (pan === null) return
              return patchTrack(track.id, { pan })
            },
          })),
          ...(["previous", "next"] as const).map((direction) => ({
            title: direction === "previous" ? t("Previous preset") : t("Next preset"),
            disabled: nextMixerPanPreset(track.pan, direction) === null,
            run: () => {
              const latest = useProjectStore.getState().project.mixer.tracks.find(
                (latest) => latest.id === track.id
              )
              if (!latest) return
              const next = nextMixerPanPreset(latest.pan, direction)
              if (next === null) return
              return patchTrack(track.id, { pan: next })
            },
          })),
          ...(["half", "double"] as const).map((factor) => ({
            title: factor === "half" ? t("Half") : t("Double"),
            disabled: nextMixerPanScale(track.pan, factor) === null,
            run: () => {
              const latest = useProjectStore.getState().project.mixer.tracks.find(
                (latest) => latest.id === track.id
              )
              if (!latest) return
              const next = nextMixerPanScale(latest.pan, factor)
              if (next === null) return
              return patchTrack(track.id, { pan: next })
            },
          })),
        ],
      },
    ],
    [track.id, track.pan]
  )
  const live = useMemo(
    () => automationFeed({ type: "trackPan", track: track.id }),
    [track.id]
  )
  const marker = useAutomationMarker({ type: "trackPan", track: track.id })
  return (
    <ValueContextItems items={items}>
      <PanControl
        size="sm"
        showValue={showValue}
        live={live}
        marker={marker}
        aria-label={message("{trackName} pan", { trackName: track.name })}
        className={cn(
          DIMMED,
          "[&_[data-slot=knob-value]]:font-readout [&_[data-slot=knob-value]]:text-[9px] [&_[data-slot=knob-value]]:text-muted-foreground"
        )}
        {...pan}
        {...hint}
      />
    </ValueContextItems>
  )
}

function MuteButtons({ track, small }: { track: StripTrack; small: boolean }) {
  const hint = useHint(
    track.current ? t("Mute the Current utility copy") : track.id === MASTER_TRACK
      ? t("Mute silences everything")
      : t("Mute silences this track. Solo leaves only it and what it needs to be heard")
  )
  const size = small ? "sm" : "md"
  if (track.id === MASTER_TRACK || track.current) {
    return (
      <ToggleLed
        size={size}
        pressed={track.muted}
        color="var(--wf-mute, var(--wf-meter-mid))"
        aria-label={t("Mute")}
        onPressedChange={(muted) => void patchTrack(track.id, { muted })}
        {...hint}
      >
        M
      </ToggleLed>
    )
  }
  return (
    <MuteSolo
      size={size}
      muted={track.muted}
      solo={track.solo}
      onMutedChange={(muted) => void patchTrack(track.id, { muted })}
      onSoloChange={(solo) => void patchTrack(track.id, { solo })}
      {...hint}
    />
  )
}

/** Where the master goes: the sound card. Opens the audio settings. */
function MasterOutput() {
  const device = useEngineStore((state) =>
    state.status?.running ? state.status.device : null
  )
  const hint = useHint(
    t("The master plays to the audio output. Click to choose a device")
  )
  return (
    <button
      type="button"
      data-slot="track-output"
      aria-label={message("Output: {device}", { device: device ?? t("no audio device") })}
      className="flex h-[18px] w-full min-w-0 items-center gap-0.5 rounded-[3px] pr-1 pl-0.5 text-[10px] leading-none text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      onClick={() => void runAction("options.settings")}
      {...hint}
    >
      <HugeiconsIcon
        icon={ArrowRight02Icon}
        strokeWidth={2}
        className="size-3 shrink-0"
      />
      <span className="truncate">{device ?? t("No device")}</span>
    </button>
  )
}

/** The output selector and the sends, inline. Every strip is equally tall. */
function Routing({ track, sendRows }: { track: StripTrack; sendRows: number }) {
  const master = track.id === MASTER_TRACK
  if (track.current) return <div className="shrink-0 px-1 text-[10px] text-muted-foreground" style={{ height: 38 + sendRows * SEND_ROW_HEIGHT }}>{t("Follows selection")}</div>
  return (
    <div
      data-slot="track-routing-inline"
      className={cn("flex shrink-0 flex-col gap-0.5 px-1", DIMMED)}
    >
      {master ? (
        <MasterOutput />
      ) : (
        <OutputSelect id={track.id} output={track.output} className="w-full" />
      )}
      <div style={{ height: sendRows * SEND_ROW_HEIGHT }}>
        {!master && (
          <SendList id={track.id} sends={track.sends} className="max-h-full" />
        )}
      </div>
      <div className="flex h-[18px]">
        {!master && <AddSendMenu id={track.id} />}
      </div>
    </div>
  )
}

function stripLabel(track: StripTrack, index: number, audible: Audibility) {
  const place = track.id === MASTER_TRACK ? t("master track") : message("track {index}", { index: plainNumber(index) })
  const state =
    audible === "muted"
      ? t(", muted")
      : audible === "silenced"
        ? t(", silenced by solo")
        : ""
  return `${track.name}, ${place}${state}`
}

const LEVEL_LAYOUT: Record<StripMode, LevelLayout> = {
  full: "tall",
  compact: "tall",
  tight: "short",
  flat: "flat",
  mini: "bare",
}

export type MixerStripProps = {
  id: TrackId
  mode: StripMode
  /** Send rows every strip keeps free in "full" mode. */
  sendRows: number
  /** Rows of the effect rack. 0 shows a badge with the count instead. */
  effectRows: number
  /** The channel selected in the rack plays into this track. */
  linked: boolean
  /** False for a strip kept mounted while it is scrolled out of view. */
  metering: boolean
}

/**
 * One mixer track. It reads its own track from the store, so moving a fader
 * renders this strip and no other, and it leaves the settings of the
 * track's effects out, so turning a knob in an effect renders no strip.
 */
export const MixerStrip = memo(function MixerStrip({
  id,
  mode,
  sendRows,
  effectRows,
  linked,
  metering,
}: MixerStripProps) {
  const track = useStripTrack(id)
  const index = useMixerTrackIndex(id)
  const number = useProjectStore((state) => state.project.mixer.tracks.filter((track) => !track.current).findIndex((track) => track.id === id))
  const selected = useUiStore((state) => state.selectedTrack === id)
  const groupSelected = useMixerUi((state) => state.selected.includes(id))
  const pointerSelecting = useRef(false)
  const heard = useProjectStore((state) =>
    heardTracks(state.project.mixer.tracks).has(id)
  )
  const drop = useEffectDrop(id)

  if (!track) return null

  const master = id === MASTER_TRACK
  const audible: Audibility = track.muted
    ? "muted"
    : heard
      ? "heard"
      : "silenced"
  // The fader lies across the strip.
  const low = mode === "flat" || mode === "mini"
  // The channel chips have a row of their own.
  const chips = mode === "full" || mode === "compact"

  // The lowest strip gives the name row to the name and the effect badge,
  // so the routing button moves down beside mute and solo.
  const mini = mode === "mini"
  const routing =
    !track.current && mode !== "full" && (!master || !chips) ? (
      <RoutingButton track={track} withChannels={!chips} />
    ) : null

  function select() {
    const ui = useUiStore.getState()
    if (ui.selectedTrack !== id) ui.selectTrack(id)
  }

  return (
    <div
      role="group"
      aria-label={stripLabel(track, index, audible)}
      data-track={id}
      data-mode={mode}
      data-audible={audible}
      data-selected={selected || undefined}
      data-group-selected={groupSelected || undefined}
      data-linked={linked ? "" : undefined}
      data-drop={drop.gap === null ? undefined : ""}
      tabIndex={0}
      onPointerDownCapture={(event) => { pointerSelecting.current = true; setTimeout(() => { pointerSelecting.current = false }, 0); selectMixerTrack(id, event.button === 0 && (event.ctrlKey || event.metaKey), event.button === 0 && event.shiftKey) }}
      onFocus={() => { if (!pointerSelecting.current) select() }}
      {...drop.zone}
      className={cn(
        // `outline-none` alone would switch the focus outline off for good:
        // the width set on focus needs its style said again.
        "group/strip relative flex h-full w-full flex-col border-r outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring focus-visible:outline-solid",
        low ? "gap-0.5" : "gap-1.5 pb-1.5",
        "data-linked:bg-[color-mix(in_oklch,var(--wf-brand)_10%,transparent)]",
        "data-selected:bg-accent data-selected:shadow-[inset_0_-2px_0_var(--wf-brand)]",
        "data-group-selected:bg-accent/60 data-group-selected:shadow-[inset_0_-2px_0_var(--wf-brand)]",
        // An effect dragged over the strip would land on this track.
        "data-drop:bg-[color-mix(in_oklch,var(--wf-brand)_9%,transparent)]"
      )}
    >
      <StripHeader
        id={id}
        name={track.name}
        color={track.color}
        number={track.current ? "C" : master ? "M" : String(number)}
        master={master}
        trailing={mini ? <EffectBadge track={id} showEmpty={false} /> : routing}
      />
      {chips && (
        <div className={DIMMED}>
          <ChannelChips id={id} master={master} />
        </div>
      )}
      {/* Top to bottom is the way the signal goes: what plays into the
          track, its effects, then pan and the fader. */}
      {effectRows > 0 && (
        <EffectRack
          track={id}
          rows={effectRows}
          dropGap={drop.gap}
          className={DIMMED}
        />
      )}
      {mode === "flat" && (
        <div className={cn("flex h-4 shrink-0 px-1", DIMMED)}>
          <EffectBadge track={id} showEmpty />
        </div>
      )}
      {mode === "full" && <Routing track={track} sendRows={sendRows} />}
      <div
        className={cn(
          "flex shrink-0 items-start justify-center",
          mini ? "gap-0.5" : "gap-2"
        )}
      >
        <PanKnob track={track} showValue={mode === "full"} />
        <div className="flex h-6 items-center gap-0.5">
          <MuteButtons track={track} small={low} />
          {mini && routing}
        </div>
      </div>
      <div
        className={cn(
          "flex min-h-0 flex-1 flex-col group-data-[audible=muted]/strip:opacity-55",
          DIMMED
        )}
      >
        <LevelSection
          id={id}
          name={track.name}
          volume={track.volume}
          index={index}
          metering={metering}
          layout={LEVEL_LAYOUT[mode]}
          wide={master}
        />
      </div>
    </div>
  )
})
