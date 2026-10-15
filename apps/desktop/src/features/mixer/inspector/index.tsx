import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import {
  Add01Icon,
  ArrowDown01Icon,
  ArrowExpand01Icon,
  ArrowRight01Icon,
  ArrowShrink01Icon,
  Cancel01Icon,
  SidebarRight01Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { Fragment, useEffect, useRef } from "react"
import { MixerRecordingPanel } from "../recording-input"
import { ExternalOutputPanel } from "../external-output"
import { LatencyOffsetPanel } from "../latency-offset"
import { CurrentSourcePanel } from "../current-source"
import { SidechainPanel } from "../sidechains"
import { TrackPresetsPanel } from "../track-presets"
import { TrackProcessingPanel } from "../track-processing"

import type { TrackId } from "@/bindings"
import { ActionButton } from "@/components/action-button"
import { ContextActions } from "@/components/context-actions"
import {
  chainLatencyFrames,
  formatLatency,
  useSampleRate,
} from "@/features/effects"
import {
  INSPECTOR_KEEPS,
  runAction,
  useShortcutLabel,
  useShortcutScope,
} from "@/lib/actions"
import { useHint, useProjectStore, useUiStore } from "@/lib/store"
import { colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"

import { registerMixerActions } from "../actions"
import { useEffectDrop } from "../effect-drag"
import { AddEffectMenu } from "../effect-menu"
import { MAX_EFFECT_SLOTS } from "../effect-ops"
import { DropLine } from "../effect-rack"
import { useEffectsUi } from "../effects-ui"
import { INSPECTOR_MENU } from "../menus"
import { useEffectIds } from "../strip-track"
import { EffectPanel } from "./effect-panel"

function useTrackFace(id: TrackId | null) {
  const name = useProjectStore(
    (state) =>
      state.project.mixer.tracks.find((track) => track.id === id)?.name ?? null
  )
  const color = useProjectStore(
    (state) =>
      state.project.mixer.tracks.find((track) => track.id === id)?.color ?? 0
  )
  const current = useProjectStore((state) => state.project.mixer.tracks.find((track) => track.id === id)?.current ?? false)
  return { name, color, current }
}

/** Frames the track's own effects delay it by, at the engine's rate. */
function useChainLatency(id: TrackId | null, sampleRate: number): number {
  return useProjectStore((state) => {
    const effects =
      state.project.mixer.tracks.find((track) => track.id === id)?.effects ?? []
    const builtins = effects.filter(
      (slot) =>
        !state.project.plugins?.some(
          (plugin) =>
            plugin.target.type === "effect" && plugin.target.effect === slot.id
        )
    )
    return chainLatencyFrames(builtins, sampleRate)
  })
}

/**
 * The track's shared built-in effect latency, shown when nonzero. It sits
 * in the header where there is room beside the name, and on a line of its
 * own under it in a narrow panel.
 */
function Latency({ track }: { track: TrackId }) {
  useLanguageStore((state) => state.language)
  const sampleRate = useSampleRate()
  const frames = useChainLatency(track, sampleRate)
  const hint = useHint(
    t("Built-in effects on this track add this shared delay. The engine compensates other paths to keep them aligned; extra stereo delay is intentional")
  )
  if (frames === 0) return null
  return (
    <span
      data-slot="chain-latency"
      className="order-last flex h-5 min-w-0 basis-full items-center truncate font-readout text-[9px] leading-none text-muted-foreground @min-[26rem]/inspector:order-none @min-[26rem]/inspector:h-auto @min-[26rem]/inspector:basis-auto @min-[26rem]/inspector:pl-1"
      {...hint}
    >
      {i18nMessage("{latency} compensated", { latency: formatLatency(frames, sampleRate) })}
    </span>
  )
}

function Chain({ track }: { track: TrackId }) {
  useLanguageStore((state) => state.language)
  const ids = useEffectIds(track)
  const drop = useEffectDrop(track)

  if (ids.length === 0) {
    return (
      <div
        data-slot="effect-empty"
        className="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center text-muted-foreground"
        {...drop.zone}
      >
        <p className="font-medium text-foreground">{t("No effects on this track")}</p>
        <p className="max-w-56 leading-snug">{t("Sound runs through a track's effects from the top down, before its fader. Add one, or drag one here from another track.")}</p>
        <AddEffectMenu
          track={track}
          className="h-6 gap-1 border border-dashed px-2 text-xs text-foreground"
        >
          <HugeiconsIcon icon={Add01Icon} strokeWidth={2} className="size-3" />{t("Add effect")}</AddEffectMenu>
      </div>
    )
  }

  return (
    <ul
      aria-label={t("Effect chain")}
      data-slot="effect-chain"
      // One column beside the strips. With the room of the editor area the
      // effects sit side by side, each wide enough for its wide layout.
      className="grid min-h-0 flex-1 grid-cols-[repeat(auto-fit,minmax(min(100%,34rem),1fr))] content-start overflow-x-hidden overflow-y-auto"
      {...drop.zone}
    >
      {ids.map((id, index) => (
        <Fragment key={id}>
          {drop.gap === index && <DropLine />}
          <EffectPanel track={track} effect={id} />
        </Fragment>
      ))}
      {drop.gap === ids.length && <DropLine />}
      {ids.length < MAX_EFFECT_SLOTS && (
        <li className="col-span-full flex list-none px-1.5 py-1.5">
          <AddEffectMenu track={track} className="h-5 text-[11px]">
            <HugeiconsIcon
              icon={Add01Icon}
              strokeWidth={2}
              className="size-3"
            />{t("Add effect")}</AddEffectMenu>
        </li>
      )}
    </ul>
  )
}

/**
 * The track's own settings: sidechain, presets, EQ, input, hardware output
 * and latency. They fold into one row by default so the effects, which are
 * what the inspector is for, get its height. Unfolded they scroll on their
 * own and never take more than half of it.
 */
function TrackSettings({ track, current }: { track: TrackId; current: boolean }) {
  useLanguageStore((state) => state.language)
  const open = useEffectsUi((state) => state.trackSettingsOpen)
  return (
    <section
      data-slot="track-settings"
      aria-label={t("Track settings")}
      className="flex max-h-[50%] min-h-0 shrink-0 flex-col border-b"
    >
      <button
        type="button"
        aria-expanded={open}
        data-slot="track-settings-toggle"
        className="flex h-6 shrink-0 items-center gap-1 px-2 text-left text-[11px] text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        onClick={() => useEffectsUi.getState().setTrackSettingsOpen(!open)}
      >
        <HugeiconsIcon
          icon={open ? ArrowDown01Icon : ArrowRight01Icon}
          strokeWidth={2}
          className="size-3 shrink-0"
        />
        <span className="truncate">{t("Track settings")}</span>
        {!open && (
          <span className="min-w-0 truncate text-[10px] text-muted-foreground/80">
            {current
              ? t("Sidechain, presets, EQ")
              : t("Sidechain, presets, EQ, input, output, latency")}
          </span>
        )}
      </button>
      {open && (
        // Keyed by track, as each panel keeps a draft of its own.
        <div key={track} className="min-h-0 overflow-y-auto border-t">
          <SidechainPanel track={track} />
          <TrackPresetsPanel track={track} />
          <TrackProcessingPanel track={track} />
          {!current && <MixerRecordingPanel track={track} />}
          {!current && <ExternalOutputPanel track={track} />}
          {!current && <LatencyOffsetPanel track={track} />}
        </div>
      )}
    </section>
  )
}

type EffectInspectorProps = {
  /** Shown in the editor area instead of beside the strips. */
  enlarged?: boolean
}

/**
 * The effects of the selected track: one panel per effect in chain order,
 * each with its editor. It is docked beside the strips, with the height of
 * the mixer and a scroll of its own, and can be enlarged into the editor
 * area for detailed work.
 */
export function EffectInspector({ enlarged = false }: EffectInspectorProps) {
  useLanguageStore((state) => state.language)
  const selected = useUiStore((state) => state.selectedTrack)
  const { name, color, current } = useTrackFace(selected)
  const count = useEffectIds(selected).length
  const root = useRef<HTMLElement>(null)
  // The effects are a scope of their own inside the mixer's. Delete and
  // Ctrl+D pressed in here are about an effect or a setting, never about
  // the track.
  const scope = useShortcutScope("effectInspector", { keeps: INSPECTOR_KEEPS })

  // Enlarged, it takes the keyboard, so Escape brings it back at once.
  useEffect(() => {
    if (enlarged) root.current?.focus({ preventScroll: true })
  }, [enlarged])

  return (
    <ContextActions items={INSPECTOR_MENU}>
      <aside
        ref={root}
        tabIndex={-1}
        aria-label={t("Effects")}
        data-slot="effect-inspector"
        data-enlarged={enlarged ? "" : undefined}
        // Docked, it is the bottom right corner, where toasts come up.
        data-toast-clear={enlarged ? undefined : ""}
        className="@container/inspector flex h-full min-h-0 min-w-0 flex-col bg-background outline-none"
        {...scope}
      >
        <header className="flex min-h-7 shrink-0 flex-wrap items-center gap-x-1.5 border-b bg-chassis/60 pr-1 pl-2.5">
          {name !== null && (
            <span
              aria-hidden
              className="size-2 shrink-0 rounded-[2px]"
              style={{ backgroundColor: colorToCss(color) }}
            />
          )}
          <h2
            className="min-w-0 truncate text-xs font-medium"
            title={name ?? undefined}
          >
            {name === null ? t("Effects") : i18nMessage("{name} effects", { name })}
          </h2>
          {/* Outside the name, so a long name is cut short and not this. */}
          {name !== null && count > 0 && (
            <span
              data-slot="effect-count"
              aria-label={i18nMessage("{value0} of {value1} effects", { value0: count, value1: MAX_EFFECT_SLOTS })}
              className="shrink-0 font-readout text-[9px] text-muted-foreground"
            >
              {count}/{MAX_EFFECT_SLOTS}
            </span>
          )}
          {selected !== null && name !== null && <Latency track={selected} />}
          <div className="ml-auto flex shrink-0 items-center gap-0.5">
            <ActionButton
              action="mixer.enlargeEffects"
              variant="ghost"
              size="icon-xs"
              aria-pressed={enlarged}
              className="text-muted-foreground"
              tooltipSide="left"
            >
              <HugeiconsIcon
                icon={enlarged ? ArrowShrink01Icon : ArrowExpand01Icon}
                strokeWidth={2}
              />
            </ActionButton>
            <ActionButton
              action="mixer.effects"
              variant="ghost"
              size="icon-xs"
              className="text-muted-foreground"
              tooltipSide="left"
            >
              <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
            </ActionButton>
          </div>
        </header>
        {current && <CurrentSourcePanel />}
        {selected !== null && name !== null && <TrackSettings track={selected} current={current} />}
        {selected !== null && name !== null ? (
          // Keyed by track so a drag in flight never lands on another one.
          <Chain key={selected} track={selected} />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-1 p-4 text-center text-muted-foreground">
            <p className="font-medium text-foreground">{t("No track selected")}</p>
            <p>{t("Click a strip to see its effects here.")}</p>
          </div>
        )}
      </aside>
    </ContextActions>
  )
}

/**
 * The effects in the editor area, over the tab that was showing. They are
 * still the mixer's, so the mixer's keys work in here.
 */
export function EnlargedEffects() {
  useLanguageStore((state) => state.language)
  const scope = useShortcutScope("mixer")
  useEffect(() => registerMixerActions(), [])
  return (
    <div data-slot="enlarged-effects" className="h-full min-h-0" {...scope}>
      <EffectInspector enlarged />
    </div>
  )
}

type EffectInspectorTabProps = {
  /** The effects are showing in the editor area right now. */
  enlarged?: boolean
}

/**
 * What the inspector folds down to: a narrow tab at the right edge of the
 * mixer that opens it again, or brings it back from the editor area.
 */
export function EffectInspectorTab({
  enlarged = false,
}: EffectInspectorTabProps) {
  useLanguageStore((state) => state.language)
  const action = enlarged ? "mixer.enlargeEffects" : "mixer.effects"
  const shortcut = useShortcutLabel(action)
  const what = enlarged
    ? t("The effects are in the editor area. Click to bring them back here")
    : t("Show the selected track's effects")
  const hint = useHint(shortcut ? i18nMessage("{help} ({shortcut})", { help: what, shortcut }) : what)
  return (
    <button
      type="button"
      data-slot="effect-inspector-tab"
      data-toast-clear=""
      aria-label={enlarged ? t("Bring the effects back") : t("Show effects")}
      aria-expanded={false}
      className={cn(
        "flex h-full w-[22px] shrink-0 flex-col items-center gap-1.5 border-l bg-chassis/60 pt-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
        enlarged && "text-foreground"
      )}
      onClick={() => void runAction(action)}
      {...hint}
    >
      <HugeiconsIcon
        icon={enlarged ? ArrowShrink01Icon : SidebarRight01Icon}
        strokeWidth={2}
        className="size-3.5 shrink-0"
      />
      <span className="text-[10px] leading-none [writing-mode:vertical-rl]">{t("Effects")}</span>
    </button>
  )
}
