import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useCallback, useState, useSyncExternalStore } from "react"
import { Cancel01Icon, Edit02Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { ActionButton } from "@/components/action-button"
import { ContextActions } from "@/components/context-actions"
import { INSPECTOR_KEEPS, useShortcutScope } from "@/lib/actions"
import { isInstrumentChannel, isSamplerChannel } from "@/lib/channel-source"
import { useProjectStore } from "@/lib/store/project"
import { useGesture } from "@/lib/store/gesture"
import { useChannel, useChannelCount } from "@/lib/store/selectors"
import { ChannelVoicePanel } from "../voice"
import { EchoStatus } from "../voice/echo-status"
import { useUiStore } from "@/lib/store/ui"
import { colorToCss } from "@/lib/units"

import { INSPECTOR_MENU } from "../menus"
import { EnvelopeSection } from "./envelope-section"
import { ExpressionSection } from "./expression-section"
import { InstrumentSection } from "./instrument-section"
import { KeyboardSection } from "./keyboard-section"
import { LayerSection } from "./layer-section"
import { LoopSection } from "./loop-section"
import { SamplePreprocessSection } from "./preprocess-section"
import { RoutingSection } from "./routing-section"
import { SampleSection } from "./sample-section"
import { SoundSection } from "./sound-section"
import { SamplerProcessingSection } from "./processing-section"
import { TimingSection } from "./timing-section"
import { ModularRackSection } from "@/features/modular/editor"

/** At fewer sections than this the settings are short enough without a bar. */
const NAV_MIN_SECTIONS = 4

const sectionsIn = (scroller: HTMLElement) => [
  ...scroller.querySelectorAll<HTMLElement>("[data-inspector-section]"),
]
const sectionName = (section: HTMLElement) =>
  section.dataset.inspectorSection ?? ""

/**
 * A row of the sections in the settings below, to jump to one instead of
 * scrolling through all of them. It follows the sections there are: they
 * come and go with the channel's kind and its settings.
 */
function SectionNav({ scroller }: { scroller: HTMLElement | null }) {
  useLanguageStore((state) => state.language)
  const subscribe = useCallback(
    (changed: () => void) => {
      if (!scroller) return () => {}
      const observer = new MutationObserver(changed)
      observer.observe(scroller, {
        childList: true,
        subtree: true,
        attributeFilter: ["data-inspector-section"],
      })
      return () => observer.disconnect()
    },
    [scroller]
  )
  // One string, so a look that finds the same sections changes nothing.
  const listed = useSyncExternalStore(subscribe, () =>
    scroller ? sectionsIn(scroller).map(sectionName).join("\n") : ""
  )
  const names = listed === "" ? [] : listed.split("\n")

  if (names.length < NAV_MIN_SECTIONS) return null
  return (
    <nav
      aria-label={t("Channel settings sections")}
      className="flex shrink-0 gap-1 overflow-x-auto border-b px-2 py-1 [scrollbar-width:thin]"
    >
      {names.map((name, index) => (
        <button
          key={`${index}:${name}`}
          type="button"
          aria-label={message("Go to {section}", { section: name })}
          onClick={() => {
            const target = scroller && sectionsIn(scroller)[index]
            target?.scrollIntoView?.({ block: "start", behavior: "smooth" })
          }}
          className="shrink-0 rounded-sm px-1.5 py-0.5 text-[0.6875rem] whitespace-nowrap text-muted-foreground outline-none hover:bg-foreground/5 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          {name}
        </button>
      ))}
    </nav>
  )
}

/**
 * The settings of the selected channel, docked beside the rack: a sampler's
 * sample, pitch, level and envelope, or an instrument's own panel, then a
 * keyboard to play it and its mixer track.
 */
export function ChannelInspector() {
  useLanguageStore((state) => state.language)
  const selected = useUiStore((state) => state.selectedChannel)
  const channel = useChannel(selected)
  const count = useChannelCount()
  const hasModularRack = useProjectStore((state) => state.project.settings.modularRacks?.some((rack) => rack.channel === selected) ?? false)
  // The settings are a scope of their own inside the rack's. Delete and
  // Ctrl+D pressed in here are about a setting, never about the channel.
  const scope = useShortcutScope("rackInspector", { keeps: INSPECTOR_KEEPS })
  // One knob drag in the voice tools is one undo step.
  const voiceGesture = useGesture()
  // The scrolling settings, for the section bar to look through.
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)

  return (
    <ContextActions items={INSPECTOR_MENU}>
      <aside
        aria-label={t("Channel settings")}
        className="flex h-full min-h-0 min-w-0 flex-col bg-background"
        {...scope}
      >
        <header className="flex h-[22px] shrink-0 items-center gap-1.5 border-b pr-0.5 pl-2.5">
          {channel && (
            <span
              aria-hidden
              className="size-2 shrink-0 rounded-[2px]"
              style={{ backgroundColor: colorToCss(channel.color) }}
            />
          )}
          <h2
            className="min-w-0 flex-1 truncate text-xs font-medium"
            title={channel?.name}
          >
            {channel ? channel.name : t("Channel settings")}
          </h2>
          {channel && (
            <ActionButton
              action="channel.rename"
              variant="ghost"
              size="icon-xs"
              className="text-muted-foreground"
            >
              <HugeiconsIcon icon={Edit02Icon} strokeWidth={2} />
            </ActionButton>
          )}
          <ActionButton
            action="channelRack.settings"
            variant="ghost"
            size="icon-xs"
            className="text-muted-foreground"
            tooltipSide="left"
          >
            <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
          </ActionButton>
        </header>
        {channel && <SectionNav scroller={scroller} />}
        {channel ? (
          // Keyed by channel so a drag in flight never lands on another one.
          <div
            key={channel.id}
            ref={setScroller}
            className="min-h-0 flex-1 overflow-y-auto"
          >
            {isSamplerChannel(channel) ? (
              <>
                <SampleSection channel={channel} />
                <LoopSection channel={channel} />
                <SamplePreprocessSection channel={channel} />
                <SamplerProcessingSection channel={channel} />
                <SoundSection channel={channel} />
                <EnvelopeSection channel={channel} />
              </>
            ) : (
              isInstrumentChannel(channel) && !hasModularRack && (
                <InstrumentSection channel={channel} />
              )
            )}
            <LayerSection channel={channel} />
            <ModularRackSection channel={channel} />
            <ExpressionSection channel={channel} />
            {/* The panel shows the saved settings whenever none of its own
                edits is on its way, so undo and redo show through. */}
            <ChannelVoicePanel
              initialSettings={channel.voice}
              kind={isSamplerChannel(channel) ? "sampler" : "instrument"}
              onGestureStart={voiceGesture.begin}
              onGestureEnd={voiceGesture.end}
              onChange={(settings) =>
                voiceGesture.dispatch({
                  type: "setChannelVoiceSettings",
                  id: channel.id,
                  settings,
                })
              }
            />
            <KeyboardSection channel={channel} />
            <EchoStatus />
            <TimingSection channel={channel} />
            <RoutingSection channel={channel} />
          </div>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-1 p-4 text-center text-muted-foreground">
            <p className="font-medium text-foreground">{t("No channel selected")}</p>
            <p>
              {count === 0
                ? t("Add a channel to shape its sound here.")
                : t("Click a channel's name to shape its sound here.")}
            </p>
          </div>
        )}
      </aside>
    </ContextActions>
  )
}
