import { Alert02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { memo, type KeyboardEvent, type PointerEvent } from "react"

import type { ChannelId } from "@/bindings"
import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import {
  Popover,
  PopoverContent,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { useHint } from "@/lib/store/hint"
import { usePluginBinding } from "@/features/plugins/store"
import { colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"

import { FILL_INTERVALS } from "./actions"
import { useAudition } from "./audition"
import {
  replaceSampleFromPickedFile,
  selectChannel,
  setChannelColor,
  toggleMute,
  toggleSolo,
} from "./channel-ops"
import { ColorSwatches } from "./color-swatches"
import { useRackStore } from "./rack-store"
import { channelGroupItems } from "./channel-groups"
import { WaveGlyph, type WaveShape } from "./synth/wave-glyph"
import { translatedText as t, message, dynamicTitle } from "@/lib/i18n"

type ChannelButtonProps = {
  id: ChannelId
  name: string
  color: number
  selected: boolean
  muted: boolean
  solo: boolean
  /** Muted, or silent because another channel is soloed. */
  dimmed: boolean
  /**
   * The wave to draw for a channel that plays an instrument, which has no
   * sample to show or miss. Null for a sampler.
   */
  instrument: WaveShape | null
  /**
   * What the channel lacks to make a sound: a sampler's sample, a
   * SoundFont, or another sample-based instrument's source. Null when it
   * has its sound.
   */
  missing: "sample" | "soundFont" | "source" | null
  /** The channel has a sample, but its file could not be read. */
  sampleMissing: boolean
  /**
   * A dragged sample is over the button: it would replace the channel's
   * sample, or, on an instrument, be refused.
   */
  drop: "replace" | "refuse" | null
}

/** Finds the control to move to with the arrow keys, in this row or the next. */
function neighbor(
  from: HTMLElement,
  selector: string,
  rows: -1 | 0 | 1
): HTMLElement | null {
  let row = from.closest<HTMLElement>("[data-channel-row]")
  if (rows !== 0) {
    const next =
      rows < 0 ? row?.previousElementSibling : row?.nextElementSibling
    row = next instanceof HTMLElement && next.dataset.channelRow ? next : null
  }
  return row?.querySelector<HTMLElement>(selector) ?? null
}

function ColorChip({
  id,
  name,
  color,
}: Pick<ChannelButtonProps, "id" | "name" | "color">) {
  const open = useRackStore((state) => state.colorPickerFor === id)
  const openColorPicker = useRackStore((state) => state.openColorPicker)
  const hint = useHint(t("Channel color: click to choose another"))

  return (
    <Popover
      open={open}
      onOpenChange={(next) => openColorPicker(next ? id : null)}
    >
      <PopoverTrigger
        render={
          <button
            type="button"
            // Kept out of the tab order, which is long enough with one stop
            // per control per row. "Change channel color" reaches it too.
            tabIndex={-1}
            aria-label={message("{name} color", { name })}
            className="h-full w-2 shrink-0 rounded-l-[3px] outline-none hover:brightness-125 focus-visible:ring-2 focus-visible:ring-ring"
            style={{ backgroundColor: colorToCss(color) }}
            {...hint}
          />
        }
      />
      <PopoverContent align="start" className="w-auto gap-2 p-2">
        <PopoverTitle className="text-xs font-medium">
          {t("Color of")}{" "}{name}
        </PopoverTitle>
        <ColorSwatches
          value={color}
          onPick={(picked) => {
            void setChannelColor(id, picked)
            openColorPicker(null)
          }}
        />
      </PopoverContent>
    </Popover>
  )
}

/**
 * The name plate of a row. Pressing it plays the channel, clicking it
 * selects the channel and opens its settings, and right-clicking lists what
 * can be done to the channel.
 */
export const ChannelButton = memo(function ChannelButton({
  id,
  name,
  color,
  selected,
  muted,
  solo,
  dimmed,
  instrument,
  missing,
  sampleMissing,
  drop,
}: ChannelButtonProps) {
  const audition = useAudition(id)
  const plugin = usePluginBinding({ type: "instrument", channel: id })
  // A hosted plugin makes the sound, whatever the built-in would need.
  const hasSample = missing === null || Boolean(plugin)
  const badgeHint = useHint(
    missing === "soundFont"
      ? message("{name} has no SoundFont loaded, so it is silent. Click to choose one", { name })
      : missing === "sample"
        ? message("{name} has no sample, so it is silent. Click to choose one", { name })
        : message("{name} has no sound loaded, so it is silent. Click to open its settings and load one", { name })
  )
  const hint = useHint(
    instrument !== null
      ? message("{name}: press and hold to hear it, click to open its settings, right-click for more", { name })
      : sampleMissing
        ? message("{name}: its sample file is missing, so it is silent. Drop another sample here, or put the file back and use File > Reload missing samples", { name })
        : hasSample
          ? message("{name}: press to hear it, click to open its settings, right-click for more. Drop a sample here to replace its sound", { name })
          : message("{name} has no sample yet. Drop one here from the browser, or click to open its settings", { name })
  )

  // Mute and solo are worded for this row. Everything else is a registry
  // action on the selected channel, which a right-click makes this one.
  const items: ContextItem[] = [
    "channel.rename",
    "channel.color",
    "channel.duplicate",
    { submenu: t("Channel group"), items: [{ dynamic: () => channelGroupItems(id) }] },
    contextSeparator,
    // Greyed out on an instrument, with the reason beside it.
    "channel.replaceSample",
    ...(instrument !== null && !plugin ? ["channel.initInstrument"] : []),
    contextSeparator,
    { ...dynamicTitle(muted ? "Unmute" : "Mute", {}), run: () => toggleMute(id) },
    { ...dynamicTitle(solo ? "Unsolo" : "Solo", {}), run: () => toggleSolo(id) },
    "channel.unmuteAll",
    "channel.unsoloAll",
    "channel.resetLevels",
    "channel.selectNextMuted",
    "channel.selectNextSolo",
    contextSeparator,
    "channel.clearSteps",
    "channel.sendStepsToPianoRoll",
    "channelRack.graph",
    ...FILL_INTERVALS.map((every) => `channel.fill${every}`),
    "channel.advancedFill",
    "channel.shiftLeft",
    "channel.shiftRight",
    contextSeparator,
    "channel.moveUp",
    "channel.moveDown",
    contextSeparator,
    "channel.routeToNewTrack",
    "channel.showInMixer",
    contextSeparator,
    "channel.delete",
  ]

  function onPointerDown(event: PointerEvent<HTMLButtonElement>) {
    if (event.button === 0) audition.start()
  }

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
    let target: HTMLElement | null
    if (event.key === "ArrowUp") {
      target = neighbor(event.currentTarget, "[data-channel-button]", -1)
    } else if (event.key === "ArrowDown") {
      target = neighbor(event.currentTarget, "[data-channel-button]", 1)
    } else if (event.key === "ArrowRight") {
      target = neighbor(
        event.currentTarget,
        '[data-slot="step-grid"] button[tabindex="0"]',
        0
      )
    } else {
      return
    }
    event.preventDefault()
    target?.focus()
  }

  return (
    <div
      className={cn(
        "flex h-5.5 min-w-0 rounded-[3px] bg-(--wf-step-off) ring-1 ring-transparent",
        selected && "bg-(--wf-step-off-alt) ring-foreground/45",
        !hasSample &&
          "bg-transparent outline-1 -outline-offset-1 outline-foreground/30 outline-dashed",
        drop === "replace" && "ring-2 ring-brand",
        drop === "refuse" && "ring-2 ring-warn"
      )}
    >
      <ColorChip id={id} name={name} color={color} />
      <ContextActions items={items}>
        <button
          type="button"
          data-channel-button={id}
          aria-pressed={selected}
          aria-label={
            instrument !== null
              ? name
              : sampleMissing
                ? message("{name}, sample file is missing", { name })
                : hasSample
                  ? name
                  : message("{name}, no sample", { name })
          }
          onClick={() => selectChannel(id, { openSettings: true })}
          onContextMenu={() => selectChannel(id)}
          onPointerDown={onPointerDown}
          onPointerUp={audition.stop}
          onPointerLeave={(event) => {
            audition.stop()
            hint.onPointerLeave()
            event.currentTarget.removeAttribute("title")
          }}
          onPointerCancel={audition.stop}
          onPointerEnter={(event) => {
            hint.onPointerEnter()
            // A name that does not fit gets the browser's tooltip.
            const label = event.currentTarget.firstElementChild
            if (label && label.scrollWidth > label.clientWidth) {
              event.currentTarget.title = name
            }
          }}
          onFocus={hint.onFocus}
          onBlur={() => {
            audition.stop()
            hint.onBlur()
          }}
          onKeyDown={onKeyDown}
          className={cn(
            "flex min-w-0 flex-1 items-center gap-1.5 rounded-r-[3px] pr-1.5 pl-1.5 text-left outline-none hover:bg-foreground/8 focus-visible:ring-2 focus-visible:ring-ring active:bg-foreground/12",
            (dimmed || !hasSample) && "text-muted-foreground"
          )}
        >
          <span className="min-w-0 flex-1 truncate font-medium">{name}</span>
          {drop === "replace" ? (
            <span className="shrink-0 text-[0.625rem] text-brand">{t("replace")}</span>
          ) : drop === "refuse" ? (
            <span className="shrink-0 text-[0.625rem] text-warn">
              {t("takes no sample")}
            </span>
          ) : instrument !== null ? (
            <WaveGlyph shape={instrument} className="text-muted-foreground" />
          ) : sampleMissing ? (
            <span
              data-sample-missing
              title={t("Sample file is missing")}
              className="flex shrink-0 items-center text-warn"
            >
              <HugeiconsIcon
                icon={Alert02Icon}
                strokeWidth={2}
                className="size-3.5"
              />
            </span>
          ) : null}
        </button>
      </ContextActions>
      {drop === null && !hasSample && (
        <button
          type="button"
          data-load-source={id}
          tabIndex={-1}
          aria-label={message("Load a sound into {name}", { name })}
          onClick={() => {
            // A sampler takes a file straight away; an instrument's own
            // loader is in its settings.
            if (missing === "sample") {
              selectChannel(id)
              void replaceSampleFromPickedFile(id)
            } else {
              selectChannel(id, { openSettings: true })
            }
          }}
          className="shrink-0 rounded-r-[3px] px-1 text-[0.625rem] text-warn underline-offset-2 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
          {...badgeHint}
        >
          {missing === "soundFont" ? t("no SoundFont") : t("no sample")}
        </button>
      )}
    </div>
  )
})
