import { localizeBackendText } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useState, type DragEvent } from "react"

import type { SampleId } from "@/bindings"
import { ActionButton } from "@/components/action-button"
import { formatMs, Waveform } from "@/components/audio"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import type { SamplerChannel } from "@/lib/channel-source"
import { hasSampleDrag, readSampleDrag } from "@/lib/dnd"
import { hasSelectionDrag, readSelectionDrag, transferSelection } from "@/features/audio-editor/transfer"
import { useHint } from "@/lib/store/hint"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useSample } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"
import { formatSampleRate } from "@/lib/time"
import { clamp, colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"

import {
  assignProjectSample,
  findChannel,
  replaceSampleFromFile,
  replaceSampleFromPickedFile,
} from "../channel-ops"
import { useGestureValue } from "../use-gesture-value"
import { Section } from "./parts"
import { useSampleInfo } from "./sample-info"
import { nextSampleTrimPreset } from "./sample-trim-preset-step"
import { nextSampleTrim, SAMPLE_TRIM_PRESETS } from "./sample-trim-presets"
import { nextSampleTrimScale } from "./sample-trim-scale"
import { nextTrimStartScale } from "./trim-start-scale"

/** Lists the samples already in the project, to point the channel at one. */
function ProjectSamples({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const samples = useProjectStore((state) => state.project.samples)
  return (
    <>
      {samples.length > 0 && (
        <>
          <DropdownMenuGroup>
            <DropdownMenuLabel>{t("Samples in this project")}</DropdownMenuLabel>
            <DropdownMenuRadioGroup
              value={channel.source.sample}
              onValueChange={(id: SampleId) =>
                void assignProjectSample(channel.id, id)
              }
            >
              {samples.map((sample) => (
                <DropdownMenuRadioItem
                  key={sample.id}
                  value={sample.id}
                  closeOnClick
                >
                  <span className="truncate">{sample.name}</span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
        </>
      )}
      <DropdownMenuItem
        onClick={() => void replaceSampleFromPickedFile(channel.id)}
      >{t("From an audio file…")}</DropdownMenuItem>
      <p className="px-2 py-1.5 text-muted-foreground">
        {t("A file can also be dragged from the browser onto the waveform or onto the channel's name.")}
      </p>
    </>
  )
}

function ChooseSample({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const empty = channel.source.sample === null
  const hint = useHint(
    t("Choose the sample this channel plays, or drag one in from the browser")
  )
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant={empty ? "outline" : "ghost"}
            size="xs"
            className="shrink-0"
            {...hint}
          />
        }
      >
        {empty ? t("Choose a sample") : t("Replace")}
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-60">
        <ProjectSamples channel={channel} />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function Trim({
  channel,
  peaks,
}: {
  channel: SamplerChannel
  peaks: number[]
}) {
  useLanguageStore((state) => state.language)
  const { id, source } = channel
  // Each edge is its own control, so a drag is named after the edge it moved.
  const start = useGestureValue(source.start, (value, dispatch) =>
    dispatch({
      type: "updateSampler",
      id,
      patch: { start: clamp(value, 0, 1) },
    })
  )
  const end = useGestureValue(source.end, (value, dispatch) =>
    dispatch({
      type: "updateSampler",
      id,
      patch: { end: clamp(value, 0, 1) },
    })
  )
  const hint = useHint(
    t("Drag the two handles to trim where the sample starts and ends. Double-click a handle to put it back")
  )

  return (
    <Waveform
      aria-label={t("Sample waveform")}
      peaks={peaks}
      color={colorToCss(channel.color)}
      start={start.value}
      end={end.value}
      onStartChange={start.onValueChange}
      onEndChange={end.onValueChange}
      onGestureStart={() => {
        start.onGestureStart()
        end.onGestureStart()
      }}
      onGestureEnd={() => {
        start.onGestureEnd()
        end.onGestureEnd()
      }}
      className="h-full"
      {...hint}
    />
  )
}

/** The sample a channel plays: its waveform, trim handles and a way to change it. */
export function SampleSection({ channel }: { channel: SamplerChannel }) {
  useLanguageStore((state) => state.language)
  const { id, source } = channel
  const sample = useSample(channel.source.sample)
  const state = useSampleInfo(sample)
  const browserVisible = useUiStore((state) => state.panels.browser)
  const [over, setOver] = useState(false)

  function onDragOver(event: DragEvent<HTMLDivElement>) {
    if (!hasSampleDrag(event) && !hasSelectionDrag(event)) return
    event.preventDefault()
    event.stopPropagation()
    event.dataTransfer.dropEffect = "copy"
    setOver(true)
  }

  function onDrop(event: DragEvent<HTMLDivElement>) {
    if (!hasSampleDrag(event) && !hasSelectionDrag(event)) return
    event.preventDefault()
    event.stopPropagation()
    setOver(false)
    const selection=readSelectionDrag(event)
    if (selection) {void transferSelection(selection,{type:"sampler",channel:channel.id});return}
    const dropped = readSampleDrag(event)
    if (dropped)
      void replaceSampleFromFile(channel.id, dropped.path, dropped.browser)
  }

  const info = state?.status === "ready" ? state.info : null

  return (
    <Section title={t("Sample")} aside={<ChooseSample channel={channel} />}>
      {info?.sourceWarnings?.map((warning) => <Alert key={warning}><AlertDescription>{localizeBackendText(warning)}</AlertDescription></Alert>)}
      <div
        data-slot="sample-drop"
        data-over={over ? "" : undefined}
        onDragOver={onDragOver}
        onDragLeave={() => setOver(false)}
        onDrop={onDrop}
        className={cn(
          "relative h-20 rounded-sm ring-1 ring-transparent",
          over && "ring-2 ring-brand"
        )}
      >
        {info ? (
          <Trim channel={channel} peaks={info.peaks} />
        ) : (
          <div
            role={state?.status === "error" ? "alert" : undefined}
            className="flex h-full flex-col items-center justify-center gap-1.5 rounded-sm border border-dashed px-3 text-center text-muted-foreground"
          >
            {sample === undefined ? (
              <>
                <p>
                  <span className="font-medium text-warn">{t("No sample.")}</span>{" "}{t("Drag one here from the browser.")}</p>
                {!browserVisible && (
                  <ActionButton
                    action="view.browser"
                    variant="outline"
                    size="xs"
                  >{t("Show the browser")}</ActionButton>
                )}
              </>
            ) : state?.status === "error" ? (
              <>
                <p>
                  <span className="font-medium text-warn">{t("The sample file is missing or cannot be read.")}</span>{" "}
                  {localizeBackendText(state.message)}
                </p>
                <ActionButton
                  action="file.reloadSamples"
                  variant="outline"
                  size="xs"
                />
              </>
            ) : (
              <p>{t("Reading the waveform…")}</p>
            )}
          </div>
        )}
        {over && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center rounded-sm bg-background/80 font-medium text-brand">{t("Drop to use this sample")}</div>
        )}
      </div>
      <div className="flex flex-wrap gap-1">
        {SAMPLE_TRIM_PRESETS.map((preset) => (
          <Button
            key={preset.label}
            variant="outline"
            size="xs"
            disabled={
              nextSampleTrim(source.start ?? 0, source.end ?? 1, preset) ===
              null
            }
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              if (
                nextSampleTrim(latest.start ?? 0, latest.end ?? 1, preset) ===
                null
              )
                return
              void dispatch({
                type: "updateSampler",
                id,
                patch: { start: preset.start, end: preset.end },
              })
            }}
          >
            {t(preset.label)}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button
            key={direction}
            variant="outline"
            size="xs"
            aria-label={
              direction === "previous"
                ? t("Choose the previous sample trim preset")
                : t("Choose the next sample trim preset")
            }
            disabled={
              nextSampleTrimPreset(
                source.start ?? 0,
                source.end ?? 1,
                direction
              ) === null
            }
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextSampleTrimPreset(
                latest.start ?? 0,
                latest.end ?? 1,
                direction
              )
              if (next === null) return
              void dispatch({
                type: "updateSampler",
                id,
                patch: { start: next.start, end: next.end },
              })
            }}
          >
            {direction === "previous" ? t("Previous") : t("Next")}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="xs"
            aria-label={
              factor === "half" ? t("Halve sample trim") : t("Double sample trim")
            }
            disabled={
              nextSampleTrimScale(source.start ?? 0, source.end ?? 1, factor) ===
              null
            }
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextSampleTrimScale(
                latest.start ?? 0,
                latest.end ?? 1,
                factor
              )
              if (next === null) return
              void dispatch({
                type: "updateSampler",
                id,
                patch: { end: next },
              })
            }}
          >
            {factor === "half" ? t("Halve trim") : t("Double trim")}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button
            key={factor}
            variant="outline"
            size="xs"
            aria-label={
              factor === "half"
                ? t("Halve sample trim from the end")
                : t("Double sample trim from the end")
            }
            disabled={
              nextTrimStartScale(source.start ?? 0, source.end ?? 1, factor) ===
              null
            }
            onClick={() => {
              const latest = findChannel(id)?.source
              if (latest?.type !== "sampler") return
              const next = nextTrimStartScale(
                latest.start ?? 0,
                latest.end ?? 1,
                factor
              )
              if (next === null) return
              void dispatch({
                type: "updateSampler",
                id,
                patch: { start: next },
              })
            }}
          >
            {factor === "half" ? t("Halve from end") : t("Double from end")}
          </Button>
        ))}
      </div>
      {sample && (
        <div className="flex items-baseline gap-3">
          <span
            className="min-w-0 flex-1 truncate font-medium"
            title={sample.name}
          >
            {sample.name}
          </span>
          {info && (
            <span className="flex shrink-0 gap-2.5 font-readout text-[0.625rem] text-muted-foreground">
              <span>{formatMs(info.durationSecs * 1000)}</span>
              <span>{formatSampleRate(info.sampleRate)}</span>
              <span>{info.channels === 1 ? "mono" : "stereo"}</span>
            </span>
          )}
        </div>
      )}
    </Section>
  )
}
