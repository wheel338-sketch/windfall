import {
  Add01Icon,
  ArrowDown01Icon,
  SidebarRight01Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { ActionButton } from "@/components/action-button"
import { ActionMenuItem } from "@/components/action-menu-item"
import { ContextActions } from "@/components/context-actions"
import { Knob, NumberField, percentUnit } from "@/components/audio"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useActions, useAppState } from "@/lib/actions"
import { currentPatternId } from "@/lib/flows/edit"
import { useHint } from "@/lib/store/hint"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useSelectedPatternId } from "@/lib/store/selectors"
import { clamp, DEFAULT_PATTERN_STEPS, MAX_PATTERN_STEPS } from "@/lib/units"

import { LENGTH_PRESETS } from "./actions"
import { setPatternLength } from "./channel-ops"
import { RACK_MENU } from "./menus"
import { nextPatternLengthPreset } from "./pattern-length-preset-step"
import { nextPatternLengthScale } from "./pattern-length-scale"
import { nextProjectSwingPreset } from "./project-swing-preset-step"
import { useRackStore } from "./rack-store"
import { clampPatternLength, describeLength } from "./steps"
import { nextSwing, SWING_PRESETS } from "./swing-presets"
import { nextProjectSwingScale } from "./swing-scale"
import { useGestureValue } from "./use-gesture-value"
import { ChannelGroupFilter } from "./channel-groups-ui"
import { translatedText as t, message, plainNumber, dynamicTitle } from "@/lib/i18n"

function AddChannelMenuItems() {
  const actions = useActions()
  const state = useAppState()
  const add = actions.find((action) => action.id === "channel.add")
  const instruments = actions.filter((action) =>
    action.id.startsWith("channel.addInstrument.")
  )
  const layer = actions.find((action) => action.id === "channel.addLayer")
  const fromFile = actions.find((action) => action.id === "channel.addFromFile")
  const browser = actions.find((action) => action.id === "view.browser")

  return (
    <>
      {add && <ActionMenuItem action={add} state={state} />}
      {instruments.map((action) => (
        <ActionMenuItem key={action.id} action={action} state={state} />
      ))}
      {layer && <ActionMenuItem action={layer} state={state} />}
      {fromFile && <ActionMenuItem action={fromFile} state={state} />}
      <DropdownMenuSeparator />
      <p className="px-2 py-1.5 text-muted-foreground">
        {t("A sound can also be dragged from the browser onto the rack. Drop it on a sampler's name to replace that channel's sound.")}
      </p>
      {browser && !state.ui.panels.browser && (
        <ActionMenuItem action={browser} state={state} />
      )}
    </>
  )
}

function AddChannelMenu() {
  const hint = useHint(
    t("Add a channel: an empty sampler or a synth, or drag a sample in from the browser")
  )
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            size="sm"
            aria-label={t("Add channel menu")}
            {...hint}
          />
        }
      >
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
        {t("Add channel")}
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-64">
        <AddChannelMenuItems />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function PatternLength() {
  const pattern = useSelectedPatternId()
  const lengthSteps = useProjectStore(
    (state) =>
      state.project.patterns.find((item) => item.id === pattern)?.lengthSteps ??
      DEFAULT_PATTERN_STEPS
  )
  const signature = useProjectStore(
    (state) => state.project.patterns.find((item) => item.id === pattern)?.timeSignature ?? state.project.settings.timeSignature
  )
  const timeline = useProjectStore((state) => state.project.patterns.find((item) => item.id === pattern)?.timeline)
  const length = useGestureValue(lengthSteps, async (value, dispatch) => {
    if (pattern === null) return
    await dispatch({
      type: "updatePattern",
      id: pattern,
      patch: { lengthSteps: clampPatternLength(value) },
    })
  })
  const hint = useHint(
    message("Pattern length: {lengthValue} steps, {length}. Drag up or down, or double-click to type", { lengthValue: plainNumber(length.value), length: describeLength(length.value, signature, timeline) })
  )

  return (
    <div
      role="group"
      aria-label={t("Pattern length")}
      className="flex items-center gap-1"
    >
      <span className="mr-0.5 text-muted-foreground">{t("Length")}</span>
      <div className="flex items-center rounded-md bg-(--wf-step-off)/60 p-px">
        {LENGTH_PRESETS.map((steps) => (
          <ActionButton
            key={steps}
            action={`pattern.length${steps}`}
            variant={steps === lengthSteps ? "secondary" : "ghost"}
            size="xs"
            aria-pressed={steps === lengthSteps}
            className="w-7 px-0 font-readout text-[0.6875rem] aria-pressed:bg-background aria-pressed:shadow-xs"
          >
            {steps}
          </ActionButton>
        ))}
      </div>
      <Button
        variant="outline"
        size="xs"
        aria-label={t("Choose the previous pattern length preset")}
        disabled={
          nextPatternLengthPreset(
            lengthSteps ?? DEFAULT_PATTERN_STEPS,
            "previous"
          ) === null
        }
        onClick={() => {
          const id = currentPatternId()
          if (id === null) return
          const current = useProjectStore
            .getState()
            .project.patterns.find((item) => item.id === id)
          const next = nextPatternLengthPreset(
            current?.lengthSteps ?? DEFAULT_PATTERN_STEPS,
            "previous"
          )
          if (next === null) return
          void setPatternLength(next)
        }}
      >
        {t("Previous")}
      </Button>
      <Button
        variant="outline"
        size="xs"
        aria-label={t("Choose the next pattern length preset")}
        disabled={
          nextPatternLengthPreset(
            lengthSteps ?? DEFAULT_PATTERN_STEPS,
            "next"
          ) === null
        }
        onClick={() => {
          const id = currentPatternId()
          if (id === null) return
          const current = useProjectStore
            .getState()
            .project.patterns.find((item) => item.id === id)
          const next = nextPatternLengthPreset(
            current?.lengthSteps ?? DEFAULT_PATTERN_STEPS,
            "next"
          )
          if (next === null) return
          void setPatternLength(next)
        }}
      >
        {t("Next")}
      </Button>
      <NumberField
        size="sm"
        aria-label={t("Pattern length in steps")}
        min={1}
        max={MAX_PATTERN_STEPS}
        step={1}
        defaultValue={DEFAULT_PATTERN_STEPS}
        unit={<span className="text-muted-foreground">{t("steps")}</span>}
        className="font-readout"
        {...length}
        {...hint}
      />
      <Button
        variant="outline"
        size="xs"
        disabled={
          pattern === null || nextPatternLengthScale(lengthSteps, "half") === null
        }
        onClick={() => {
          if (pattern === null) return
          const current = useProjectStore
            .getState()
            .project.patterns.find((item) => item.id === pattern)
          if (!current) return
          const next = nextPatternLengthScale(current.lengthSteps, "half")
          if (next !== null) void setPatternLength(next)
        }}
      >
        {t("Half")}
      </Button>
      <Button
        variant="outline"
        size="xs"
        disabled={
          pattern === null || nextPatternLengthScale(lengthSteps, "double") === null
        }
        onClick={() => {
          if (pattern === null) return
          const current = useProjectStore
            .getState()
            .project.patterns.find((item) => item.id === pattern)
          if (!current) return
          const next = nextPatternLengthScale(current.lengthSteps, "double")
          if (next !== null) void setPatternLength(next)
        }}
      >
        {t("Double")}
      </Button>
    </div>
  )
}

function Swing() {
  const stored = useProjectStore((state) => state.project.settings.swing)
  const swing = useGestureValue(stored, (value, dispatch) =>
    dispatch({
      type: "updateSettings",
      patch: { swing: clamp(value, 0, 1) },
    })
  )
  const hint = useHint(
    message("Swing: {format}. Delays every second step for a shuffle feel. Double-click to turn it off", { format: percentUnit.format(swing.value) })
  )

  return (
    <div className="flex items-center gap-1.5" {...hint}>
      <span className="text-muted-foreground">{t("Swing")}</span>
      <Knob
        size="sm"
        aria-label={t("Swing")}
        min={0}
        max={1}
        defaultValue={0}
        {...percentUnit}
        {...swing}
      />
      <ContextActions
        items={() => [
          ...SWING_PRESETS.map((preset) => ({
            title: preset.label,
            disabled:
              nextSwing(
                useProjectStore.getState().project.settings.swing,
                preset
              ) === null,
            run: () => {
              const value = nextSwing(
                useProjectStore.getState().project.settings.swing,
                preset
              )
              if (value === null) return
              return dispatch({ type: "updateSettings", patch: { swing: value } })
            },
          })),
          {
            ...dynamicTitle("Previous preset", {}),
            disabled:
              nextProjectSwingPreset(
                useProjectStore.getState().project.settings.swing,
                "previous"
              ) === null,
            run: () => {
              const next = nextProjectSwingPreset(
                useProjectStore.getState().project.settings.swing,
                "previous"
              )
              if (next === null) return
              return dispatch({ type: "updateSettings", patch: { swing: next } })
            },
          },
          {
            ...dynamicTitle("Next preset", {}),
            disabled:
              nextProjectSwingPreset(
                useProjectStore.getState().project.settings.swing,
                "next"
              ) === null,
            run: () => {
              const next = nextProjectSwingPreset(
                useProjectStore.getState().project.settings.swing,
                "next"
              )
              if (next === null) return
              return dispatch({ type: "updateSettings", patch: { swing: next } })
            },
          },
          {
            ...dynamicTitle("Halve", {}),
            disabled:
              nextProjectSwingScale(
                useProjectStore.getState().project.settings.swing,
                "halve"
              ) === null,
            run: () => {
              const value = nextProjectSwingScale(
                useProjectStore.getState().project.settings.swing,
                "halve"
              )
              if (value === null) return
              return dispatch({ type: "updateSettings", patch: { swing: value } })
            },
          },
          {
            ...dynamicTitle("Double", {}),
            disabled:
              nextProjectSwingScale(
                useProjectStore.getState().project.settings.swing,
                "double"
              ) === null,
            run: () => {
              const value = nextProjectSwingScale(
                useProjectStore.getState().project.settings.swing,
                "double"
              )
              if (value === null) return
              return dispatch({ type: "updateSettings", patch: { swing: value } })
            },
          },
        ]}
      >
        <span className="w-7 font-readout text-[0.6875rem] text-foreground/85">
          {percentUnit.format(swing.value)}
        </span>
      </ContextActions>
    </div>
  )
}

function SettingsToggle() {
  const open = useRackStore((state) => state.inspectorOpen)
  return (
    <ActionButton
      action="channelRack.settings"
      variant={open ? "secondary" : "ghost"}
      size="icon-sm"
      aria-pressed={open}
      tooltipSide="left"
    >
      <HugeiconsIcon icon={SidebarRight01Icon} strokeWidth={2} />
    </ActionButton>
  )
}

/** The strip above the rows: add a channel, pattern length, swing. */
export function RackToolbar() {
  const graphOpen = useRackStore((state) => state.graphOpen)
  return (
    <ContextActions items={RACK_MENU}>
      <div
        role="toolbar"
        aria-label={t("Channel rack")}
        data-scroll-strip
        className="flex h-9 shrink-0 items-center gap-4 overflow-x-auto overflow-y-hidden border-b bg-chassis/40 px-1.5 whitespace-nowrap"
      >
        <AddChannelMenu />
        <ChannelGroupFilter />
        <PatternLength />
        <Swing />
        <div className="ml-auto flex items-center">
          <ActionButton action="channelRack.graph" variant={graphOpen ? "secondary" : "ghost"} size="sm" aria-pressed={graphOpen}>{t("Graph")}</ActionButton>
          <SettingsToggle />
        </div>
      </div>
    </ContextActions>
  )
}
