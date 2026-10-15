import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { pluginDropHandlers } from "@/features/plugins/insert"
import {
  ArrowDown01Icon,
  ArrowRight01Icon,
  DragDropVerticalIcon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { memo, useEffect, useRef } from "react"

import type { EffectId, EffectSlot, TrackId } from "@/bindings"
import { formatPercent, Knob, percentUnit, ToggleLed } from "@/components/audio"
import { ContextActions } from "@/components/context-actions"
import { ValueContextItems } from "@/components/value-context-menu"
import { useAutomation } from "@/features/automation/live"
import { EffectEditor } from "@/features/effects"
import { PluginControls } from "@/features/plugins/controls"
import { usePluginBinding } from "@/features/plugins/store"
import { useShortcutScope } from "@/lib/actions"
import { dispatch, useHint, useProjectStore } from "@/lib/store"
import { clamp } from "@/lib/units"

import { endEffectDrag, startEffectDrag } from "../effect-drag"
import { EffectMenuButton, effectMenu } from "../effect-menu"
import { EffectOutputs } from "../effect-outputs"
import { effectName, selectEffect, setEffectEnabled } from "../effect-ops"
import { useEffectsUi } from "../effects-ui"
import { nextMixPreset } from "../mix-preset-step"
import { MIX_PRESETS, nextMix } from "../mix-presets"
import { nextMixScale } from "../mix-scale"
import { useGestureValue } from "../use-gesture-value"

function useEffectSlot(track: TrackId, effect: EffectId) {
  return useProjectStore((state) =>
    state.project.mixer.tracks
      .find((item) => item.id === track)
      ?.effects.find((slot) => slot.id === effect)
  )
}

const clampMix = (mix: number) => (Number.isFinite(mix) ? clamp(mix, 0, 1) : 1)

/** How much of the effect is heard against the untouched signal. */
function MixKnob({ track, slot }: { track: TrackId; slot: EffectSlot }) {
  useLanguageStore((state) => state.language)
  const plugin = usePluginBinding({ type: "effect", effect: slot.id })
  const name = plugin?.name ?? effectName(slot.params.type)
  const mix = useGestureValue(
    slot.mix,
    (value) => ({
      type: "updateEffect",
      track,
      effect: slot.id,
      patch: { mix: value },
    }),
    clampMix
  )
  const hint = useHint(
    i18nMessage("Dry/wet: {value0} of the signal goes through this {value1}. Double-click for 100%", { value0: formatPercent(mix.value), value1: name })
  )
  const automation = useAutomation({
    type: "effectMix",
    track,
    effect: slot.id,
  })
  return (
    <div className="flex shrink-0 items-center gap-1" {...hint}>
      <span
        data-slot="effect-mix-value"
        className="w-7 text-right font-readout text-[9px] leading-none text-muted-foreground"
      >
        {formatPercent(mix.value)}
      </span>
      <ValueContextItems
        items={[
          ...automation.items,
          {
            submenu: t("Mix"),
            items: [
              ...MIX_PRESETS.map((preset) => ({
                title: preset.label,
                disabled: nextMix(slot.mix, preset.value) === null,
                run: () => {
                  const mix = nextMix(slot.mix, preset.value)
                  if (mix === null) return
                  return dispatch({
                    type: "updateEffect",
                    track,
                    effect: slot.id,
                    patch: { mix },
                  })
                },
              })),
              ...(["previous", "next"] as const).map((direction) => ({
                title:
                  direction === "previous" ? t("Previous preset") : t("Next preset"),
                disabled: nextMixPreset(slot.mix, direction) === null,
                run: () => {
                  const latest = useProjectStore
                    .getState()
                    .project.mixer.tracks.find((item) => item.id === track)
                    ?.effects.find((effect) => effect.id === slot.id)
                  if (!latest) return
                  const next = nextMixPreset(latest.mix, direction)
                  if (next === null) return
                  return dispatch({
                    type: "updateEffect",
                    track,
                    effect: slot.id,
                    patch: { mix: next },
                  })
                },
              })),
              ...(["halve", "double"] as const).map((factor) => ({
                title: factor === "halve" ? t("Halve") : t("Double"),
                disabled: nextMixScale(slot.mix, factor) === null,
                run: () => {
                  const latest = useProjectStore
                    .getState()
                    .project.mixer.tracks.find((item) => item.id === track)
                    ?.effects.find((effect) => effect.id === slot.id)
                  if (!latest) return
                  const next = nextMixScale(latest.mix, factor)
                  if (next === null) return
                  return dispatch({
                    type: "updateEffect",
                    track,
                    effect: slot.id,
                    patch: { mix: next },
                  })
                },
              })),
            ],
          },
        ]}
      >
        <Knob
          size="sm"
          min={0}
          max={1}
          defaultValue={1}
          aria-label={i18nMessage("{value0} dry/wet mix", { value0: name })}
          live={automation.live}
          marker={automation.marker}
          {...percentUnit}
          {...mix}
        />
      </ValueContextItems>
    </div>
  )
}

type EffectPanelProps = { track: TrackId; effect: EffectId }

/**
 * One effect in the inspector: a header with its lamp, name, dry/wet mix
 * and menu, and its editor under it. The header's grip drags the effect to
 * another place in the chain or onto another track's strip.
 */
export const EffectPanel = memo(function EffectPanel({
  track,
  effect,
}: EffectPanelProps) {
  useLanguageStore((state) => state.language)
  const slot = useEffectSlot(track, effect)
  const plugin = usePluginBinding({ type: "effect", effect })
  const collapsed = useEffectsUi((state) => state.collapsed.includes(effect))
  const selected = useEffectsUi((state) => state.selectedEffect === effect)
  const revealing = useEffectsUi((state) => state.revealing === effect)
  const root = useRef<HTMLLIElement>(null)
  const gripHint = useHint(
    t("Drag to reorder, or onto another track's strip to move it there. Ctrl copies")
  )
  // With the focus on the header, Delete and Ctrl+D mean this effect.
  const scope = useShortcutScope("effect")

  useEffect(() => {
    if (!revealing) return
    // To the top, so the editor under the header shows as well, not only
    // the header at the bottom edge.
    root.current?.scrollIntoView({ block: "start" })
    useEffectsUi.setState({ revealing: null })
  }, [revealing])

  if (!slot) return null
  const kind = slot.params.type
  const name = plugin?.name ?? effectName(kind)
  const select = () => selectEffect(effect)

  return (
    <li
      ref={root}
      data-slot="effect-panel"
      data-effect-row={effect}
      {...pluginDropHandlers({ track, effect })}
      data-kind={kind}
      data-bypassed={slot.enabled ? undefined : ""}
      data-selected={selected || undefined}
      className="group/panel relative min-w-0 list-none border-b data-selected:shadow-[inset_2px_0_0_var(--wf-brand)] [[data-enlarged]_&]:border-r"
      onPointerDownCapture={select}
      onFocusCapture={select}
    >
      <ContextActions items={effectMenu(kind, !!plugin)}>
        <header
          className="flex h-7 items-center gap-1 pr-1 pl-0.5 group-data-selected/panel:bg-accent/60"
          {...scope}
        >
          <span
            draggable
            aria-hidden
            data-slot="effect-grip"
            className="flex h-full w-3.5 shrink-0 cursor-grab items-center justify-center text-muted-foreground/70 hover:text-foreground active:cursor-grabbing"
            onDragStart={(event) => {
              startEffectDrag(event, effect)
              if (root.current) {
                event.dataTransfer.setDragImage(root.current, 12, 14)
              }
            }}
            onDragEnd={endEffectDrag}
            {...gripHint}
          >
            <HugeiconsIcon
              icon={DragDropVerticalIcon}
              strokeWidth={2}
              className="size-3"
            />
          </span>
          <ToggleLed
            variant="dot"
            size="md"
            pressed={slot.enabled}
            color="var(--wf-ok)"
            aria-label={i18nMessage("{name} on", { name })}
            onPressedChange={(on) => void setEffectEnabled(effect, on)}
          />
          <button
            type="button"
            aria-expanded={!collapsed}
            data-slot="effect-title"
            className="flex h-full min-w-0 flex-1 items-center gap-0.5 rounded-sm pl-0.5 text-left text-xs font-medium outline-none group-data-bypassed/panel:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() =>
              useEffectsUi.getState().setCollapsed(effect, !collapsed)
            }
          >
            <span className="truncate">{name}</span>
            <HugeiconsIcon
              icon={collapsed ? ArrowRight01Icon : ArrowDown01Icon}
              strokeWidth={2}
              className="size-3 shrink-0 text-muted-foreground"
            />
            {!slot.enabled && (
              <span className="ml-1 shrink-0 text-[10px] font-normal text-muted-foreground">{t("Bypassed")}</span>
            )}
          </button>
          <MixKnob track={track} slot={slot} />
          <EffectMenuButton effect={effect} kind={kind} />
        </header>
      </ContextActions>
      {!collapsed && (
        <div
          data-slot="effect-body"
          // The editors lay themselves out by the width they are given here.
          className="@container/editor px-2 pt-1 pb-1.5 transition-opacity group-data-bypassed/panel:opacity-50 in-data-enlarged:pb-2.5"
        >
          {plugin ? (
            <PluginControls binding={plugin} />
          ) : (
            <>
              <EffectEditor trackId={track} slot={slot} />
              <EffectOutputs track={track} slot={slot} />
            </>
          )}
        </div>
      )}
    </li>
  )
})
