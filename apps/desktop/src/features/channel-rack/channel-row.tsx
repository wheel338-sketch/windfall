import { pluginDropHandlers } from "@/features/plugins/insert"
import { DragDropVerticalIcon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import {
  memo,
  useCallback,
  useMemo,
  type CSSProperties,
  type KeyboardEvent,
} from "react"

import type { Channel, ChannelId, PatternId } from "@/bindings"
import {
  faderTaper,
  formatGain,
  formatPan,
  formatPercent,
  gainUnit,
  Knob,
  PanControl,
  StepGrid,
} from "@/components/audio"
import { ValueContextItems } from "@/components/value-context-menu"
import { automationFeed, useAutomationMarker } from "@/features/automation/live"
import { instrumentParams, sourceSample } from "@/lib/channel-source"
import { useGesture } from "@/lib/store/gesture"
import { useHint } from "@/lib/store/hint"
import { dispatch } from "@/lib/store/project"
import { useChannel, useLane, useSample } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"
import { colorToCss, DEFAULT_CHANNEL_VOLUME, MAX_GAIN } from "@/lib/units"
import { cn } from "@/lib/utils"

import { ChannelButton } from "./channel-button"
import { findChannel, selectChannel } from "./channel-ops"
import { nextChannelPanPreset } from "./channel-pan-preset-step"
import { nextChannelVolumePreset } from "./channel-volume-preset-step"
import { useSampleMissing } from "./inspector/sample-info"
import {
  LEFT_COLUMNS,
  LEFT_WIDTH,
  pitches,
  PITCH_VAR,
  ROW_HEIGHT,
  STEP_GAP,
  STEPS_INSET,
  STEPS_TRAIL,
} from "./layout"
import { channelTarget, channelValueItems } from "./menus"
import { MixerBadge } from "./mixer-badge"
import { MuteLamp } from "./mute-lamp"
import { RackNoteArea } from "./note-preview"
import { CHANNEL_PAN_PRESETS, nextChannelPan } from "./pan-presets"
import { nextChannelPanScale } from "./pan-scale"
import { detailSteps, litSteps } from "./steps"
import type { WaveShape } from "./synth/wave-glyph"
import { useGestureValue } from "./use-gesture-value"
import { CHANNEL_VOLUME_PRESETS, nextChannelVolume } from "./volume-presets"
import { nextChannelVolumeScale } from "./volume-scale"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"


type ChannelRowProps = {
  id: ChannelId
  pattern: PatternId
  lengthSteps: number
  /** Steps per beat, which the row shades in alternating groups. */
  groupSize: number
  beatShades?: readonly boolean[]
  /** Some channel is soloed. */
  anySolo: boolean
  /** What a sample dragged over this row's button would do, if one is. */
  drop: "replace" | "refuse" | null
}

function Grip({ id, name }: { id: ChannelId; name: string }) {
  const hint = useHint(
    t("Drag to move the channel up or down. Alt+Up and Alt+Down move the selected channel")
  )
  return (
    <span
      draggable
      data-drag-channel={id}
      aria-label={message("Move {name}", { name })}
      role="img"
      className="flex h-5 cursor-grab items-center justify-center text-muted-foreground/50 hover:text-foreground active:cursor-grabbing"
      {...hint}
    >
      <HugeiconsIcon
        icon={DragDropVerticalIcon}
        strokeWidth={2}
        className="pointer-events-none size-3"
      />
    </span>
  )
}

function Mix({
  id,
  name,
  volume,
  pan,
}: {
  id: ChannelId
  name: string
  volume: number
  pan: number
}) {
  const panControl = useGestureValue(pan, (value, dispatch) =>
    dispatch({ type: "updateChannel", id, patch: { pan: value } })
  )
  const volumeControl = useGestureValue(volume, (value, dispatch) =>
    dispatch({ type: "updateChannel", id, patch: { volume: value } })
  )
  const panHint = useHint(
    message("{name} pan: {pan}. Drag up or down, Shift for fine, double-click to center", { name, pan: formatPan(panControl.value) })
  )
  const volumeHint = useHint(
    message("{name} volume: {gain} ({percent}). Drag up or down, Shift for fine, double-click to reset", { name, gain: formatGain(volumeControl.value), percent: formatPercent(volumeControl.value) })
  )

  // What the knobs are bound to adds its own entries to their menus, and
  // automation of it moves them while the song plays.
  const panItems = useMemo(() => channelValueItems(id, "pan"), [id])
  const volumeItems = useMemo(() => channelValueItems(id, "volume"), [id])
  const panLive = useMemo(() => automationFeed(channelTarget(id, "pan")), [id])
  const volumeLive = useMemo(
    () => automationFeed(channelTarget(id, "volume")),
    [id]
  )
  const panMarker = useAutomationMarker(channelTarget(id, "pan"))
  const volumeMarker = useAutomationMarker(channelTarget(id, "volume"))

  return (
    <>
      <ValueContextItems
        items={[
          ...panItems,
          {
            submenu: t("Pan"),
            items: [
              ...CHANNEL_PAN_PRESETS.map((preset) => ({
                title: preset.label,
                disabled: nextChannelPan(pan, preset.value) === null,
                run: () => {
                  const next = nextChannelPan(pan, preset.value)
                  if (next === null) return
                  return dispatch({
                    type: "updateChannel",
                    id,
                    patch: { pan: next },
                  })
                },
              })),
              ...(["previous", "next"] as const).map((direction) => ({
                title:
                  direction === "previous" ? t("Previous preset") : t("Next preset"),
                disabled: nextChannelPanPreset(pan, direction) === null,
                run: () => {
                  const channel = findChannel(id)
                  if (!channel) return
                  const next = nextChannelPanPreset(channel.pan, direction)
                  if (next === null) return
                  return dispatch({
                    type: "updateChannel",
                    id,
                    patch: { pan: next },
                  })
                },
              })),
              ...(["half", "double"] as const).map((factor) => ({
                title: factor === "half" ? t("Half") : t("Double"),
                disabled: nextChannelPanScale(pan, factor) === null,
                run: () => {
                  const channel = findChannel(id)
                  if (!channel) return
                  const next = nextChannelPanScale(channel.pan, factor)
                  if (next === null) return
                  return dispatch({
                    type: "updateChannel",
                    id,
                    patch: { pan: next },
                  })
                },
              })),
            ],
          },
        ]}
      >
        <PanControl
          size="sm"
          aria-label={message("{name} channel pan", { name })}
          live={panLive}
          marker={panMarker}
          {...panControl}
          {...panHint}
        />
      </ValueContextItems>
      <ValueContextItems
        items={[
          ...volumeItems,
          {
            submenu: t("Volume"),
            items: [
              ...CHANNEL_VOLUME_PRESETS.map((preset) => ({
                title: preset.label,
                disabled: nextChannelVolume(volume, preset.value) === null,
                run: () => {
                  const next = nextChannelVolume(volume, preset.value)
                  if (next === null) return
                  return dispatch({
                    type: "updateChannel",
                    id,
                    patch: { volume: next },
                  })
                },
              })),
              ...(["previous", "next"] as const).map((direction) => ({
                title:
                  direction === "previous" ? t("Previous preset") : t("Next preset"),
                disabled: nextChannelVolumePreset(volume, direction) === null,
                run: () => {
                  const channel = findChannel(id)
                  if (!channel) return
                  const next = nextChannelVolumePreset(channel.volume, direction)
                  if (next === null) return
                  return dispatch({
                    type: "updateChannel",
                    id,
                    patch: { volume: next },
                  })
                },
              })),
              ...(["half", "double"] as const).map((factor) => ({
                title: factor === "half" ? t("Half") : t("Double"),
                disabled: nextChannelVolumeScale(volume, factor) === null,
                run: () => {
                  const channel = findChannel(id)
                  if (!channel) return
                  const next = nextChannelVolumeScale(channel.volume, factor)
                  if (next === null) return
                  return dispatch({
                    type: "updateChannel",
                    id,
                    patch: { volume: next },
                  })
                },
              })),
            ],
          },
        ]}
      >
        <Knob
          size="sm"
          aria-label={message("{name} channel volume", { name })}
          min={0}
          max={MAX_GAIN}
          scale={faderTaper}
          defaultValue={DEFAULT_CHANNEL_VOLUME}
          live={volumeLive}
          marker={volumeMarker}
          {...gainUnit}
          {...volumeControl}
          {...volumeHint}
        />
      </ValueContextItems>
    </>
  )
}

/** The wave of the first oscillator that sounds, to stand for a synth. */
function instrumentGlyph(channel: Channel): WaveShape | null {
  const params = instrumentParams(channel)
  if (!params || params.type !== "subtractiveSynth") return null
  const sounding = params.oscillators.find((oscillator) => oscillator.level > 0)
  return (sounding ?? params.oscillators[0]).waveform
}

/**
 * One channel: lamp, pan, volume, name, mixer track and its steps in the
 * pattern being edited. It reads only its own channel and lane from the
 * store, so an edit to one row renders that row and no other.
 */
export const ChannelRow = memo(function ChannelRow({
  id,
  pattern,
  lengthSteps,
  groupSize,
  beatShades,
  anySolo,
  drop,
}: ChannelRowProps) {
  const channel = useChannel(id)
  const lane = useLane(pattern, id)
  const selected = useUiStore((state) => state.selectedChannel === id)
  const sample = sourceSample(channel?.source)
  const sampleMissing = useSampleMissing(useSample(sample))
  const gesture = useGesture()

  const notes = lane?.notes
  const steps = useMemo(
    () => litSteps(notes, lengthSteps),
    [notes, lengthSteps]
  )
  const detail = useMemo(
    () => detailSteps(notes, lengthSteps),
    [notes, lengthSteps]
  )
  const gridStyle = useMemo<CSSProperties>(
    () => ({
      width: pitches(lengthSteps),
      gridTemplateColumns: `repeat(${lengthSteps}, calc(var(${PITCH_VAR}) - ${STEP_GAP}px))`,
      // Makes the row a whole number of step widths, so the step under the
      // pointer is found exactly.
      paddingRight: STEP_GAP,
    }),
    [lengthSteps]
  )
  const stepLabel = useCallback(
    (step: number) =>
      detail.includes(step)
        ? message("Step {index}, with notes the step grid cannot show", { index: plainNumber(step + 1) })
        : message("Step {index}", { index: plainNumber(step + 1) }),
    [detail]
  )
  const onToggle = useCallback(
    (step: number) => {
      void gesture.dispatch({ type: "toggleStep", pattern, channel: id, step })
    },
    [gesture, pattern, id]
  )
  const stepsHint = useHint(
    detail.length > 0
      ? t(t("Click a step to turn it on or off, drag to paint, right-drag to erase. Enter toggles the focused step, Space plays or stops. A dot marks notes the step grid cannot show"))
      : t(t("Click a step to turn it on or off, drag to paint, right-drag to erase. Enter toggles the focused step, Space plays or stops"))
  )

  if (!channel) return null

  const silenced = anySolo && !channel.solo && !channel.muted
  const quiet = channel.muted || silenced

  // Left from the first step goes back to the channel's name.
  function onKeyDownCapture(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowLeft" || !(event.target instanceof HTMLElement)) {
      return
    }
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
    if (event.target.dataset.step !== "0") return
    event.preventDefault()
    event.stopPropagation()
    event.currentTarget
      .querySelector<HTMLElement>("[data-channel-button]")
      ?.focus()
  }

  return (
    <div
      role="group"
      aria-label={channel.name}
      data-channel-row={id}
      {...pluginDropHandlers({ channel: id })}
      data-selected={selected ? "" : undefined}
      className="group/row flex w-max min-w-full items-center"
      style={{ height: ROW_HEIGHT }}
      onPointerDownCapture={() => {
        if (!selected) selectChannel(id)
      }}
      onFocusCapture={() => {
        if (!selected) selectChannel(id)
      }}
      onKeyDownCapture={onKeyDownCapture}
    >
      <div
        className={cn(
          LEFT_COLUMNS,
          "sticky left-0 z-10 h-full shrink-0 bg-background",
          selected && "bg-accent"
        )}
        style={{ width: LEFT_WIDTH }}
      >
        <Grip id={id} name={channel.name} />
        <MuteLamp
          id={id}
          name={channel.name}
          muted={channel.muted}
          solo={channel.solo}
          silenced={silenced}
        />
        <Mix
          id={id}
          name={channel.name}
          volume={channel.volume}
          pan={channel.pan}
        />
        <ChannelButton
          id={id}
          name={channel.name}
          color={channel.color}
          selected={selected}
          muted={channel.muted}
          dimmed={quiet}
          solo={channel.solo}
          instrument={instrumentGlyph(channel)}
          hasSample={sample !== null}
          sampleMissing={sampleMissing}
          drop={drop}
        />
        <MixerBadge
          channel={id}
          channelName={channel.name}
          track={channel.mixerTrack}
        />
      </div>
      <div
        className={cn(
          "relative flex h-full flex-1 items-center",
          selected && "bg-accent/45"
        )}
        style={{ paddingLeft: STEPS_INSET, paddingRight: STEPS_TRAIL }}
      >
        <RackNoteArea
          pattern={pattern}
          channel={id}
          name={channel.name}
          notes={notes}
          lengthSteps={lengthSteps}
          color={colorToCss(channel.color)}
          quiet={quiet}
        >
          <div className="contents" {...stepsHint}>
            <StepGrid
              steps={steps}
              color={colorToCss(channel.color)}
              groupSize={groupSize}
              beatShades={beatShades}
              aria-label={message("{channelName} steps", { channelName: channel.name })}
              stepLabel={stepLabel}
              onToggle={onToggle}
              onGestureStart={gesture.begin}
              onGestureEnd={gesture.end}
              className={cn("h-5.5", quiet && "opacity-40")}
              style={gridStyle}
            />
            {detail.map((step) => (
              <span
                key={step}
                aria-hidden
                data-detail-step={step}
                className="pointer-events-none absolute top-[5px] size-1.5 rounded-full bg-foreground ring-1 ring-background"
                style={{ left: pitches(step + 1, STEPS_INSET - STEP_GAP - 8) }}
              />
            ))}
          </div>
        </RackNoteArea>
      </div>
    </div>
  )
})
