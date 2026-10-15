import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import {
  ArrowDown01Icon,
  ArrowRight02Icon,
  Cancel01Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { Fragment, useRef, useState } from "react"
import { useShallow } from "zustand/react/shallow"

import type {
  AudioClipPatch,
  AudioClipUpdate,
  Clip,
  ClipContent,
  TrackId,
} from "@/bindings"
import {
  faderTaper,
  formatSemitones,
  gainUnit,
  Knob,
  NumberField,
  PanControl,
  parseMs,
  parseNumber,
  powerScale,
  ToggleLed,
} from "@/components/audio"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { INSPECTOR_KEEPS, useShortcutScope } from "@/lib/actions"
import { newGestureId } from "@/lib/store/gesture"
import { useHint } from "@/lib/store/hint"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"
import {
  clamp,
  colorToCss,
  MAX_GAIN,
  MAX_TUNE_SEMITONES,
  PPQ,
} from "@/lib/units"
import { cn } from "@/lib/utils"
import { AudioEditorButton } from "@/features/audio-editor"
import { AnalysisButton } from "@/features/analysis"

import { usePlaylistStore } from "../store"
import { clipFadePresetStepUpdates } from "./clip-fade-preset-step"
import { nextClipGainPreset } from "./clip-gain-preset-step"
import { nextClipPanPreset } from "./clip-pan-preset-step"
import { clipPanScaleUpdates } from "./clip-pan-scale"
import { clipPitchPresetStepUpdates } from "./clip-pitch-preset-step"
import { clipPitchScaleUpdates } from "./clip-pitch-scale"
import { FADE_PRESETS, fadePresetUpdates } from "./fade-presets"
import { fadeScaleUpdates } from "./fade-scale"
import { GAIN_PRESETS, gainPresetUpdates } from "./gain-presets"
import { gainScaleUpdates } from "./gain-scale"
import { PAN_PRESETS, panPresetUpdates } from "./pan-presets"
import { PITCH_PRESETS, pitchPresetUpdates } from "./pitch-presets"
import { describeFade, speedOf, ticksPerSecond } from "./geometry"
import { ClipProcessingControls } from "./processing-controls"
import { SliceControls } from "@/features/slicer"
import { patchSelectedAudioClips, routeSelectionToNewTrack } from "./ops"

type AudioContent = Extract<ClipContent, { type: "audio" }>
type AudioClip = Clip & { content: AudioContent }

const FADE_SCALE = powerScale(2)

/** The selected audio clips, in timeline order. */
function useSelectedAudioClips(): AudioClip[] {
  const selection = usePlaylistStore((state) => state.selection)
  return useProjectStore(
    useShallow((state) =>
      selection.size === 0
        ? []
        : state.project.playlist.clips.filter(
            (clip): clip is AudioClip =>
              selection.has(clip.id) && clip.content.type === "audio"
          )
    )
  )
}

/**
 * Connects a control to one setting of every selected audio clip. While it
 * is moved it shows its own value, and every change of one drag goes out
 * under one gesture id, so the drag is one undo step for all the clips.
 */
function useClipSetting(
  stored: number,
  patch: (value: number) => AudioClipPatch
) {
  const [local, setLocal] = useState<number | null>(null)
  const gesture = useRef<number | undefined>(undefined)
  const inFlight = useRef(0)
  const settle = () => {
    if (gesture.current === undefined && inFlight.current === 0) setLocal(null)
  }
  return {
    value: local ?? stored,
    onGestureStart() {
      gesture.current = newGestureId()
    },
    onValueChange(value: number) {
      setLocal(value)
      inFlight.current += 1
      void patchSelectedAudioClips(patch(value), gesture.current).finally(
        () => {
          inFlight.current -= 1
          settle()
        }
      )
    },
    onGestureEnd() {
      gesture.current = undefined
      settle()
    },
  }
}

/** Reads a fade typed as beats ("0.5", "2 beats") or as time ("250 ms"). */
function parseFade(text: string, tempoBpm: number): number | null {
  if (/m?s\b/i.test(text)) {
    const ms = parseMs(text)
    return ms === null ? null : (ms / 1000) * ticksPerSecond(tempoBpm)
  }
  const beats = parseNumber(text)
  return beats === null ? null : beats * PPQ
}

function Labelled({
  label,
  children,
  className,
}: {
  label: string
  children: React.ReactNode
  className?: string
}) {
  useLanguageStore((state) => state.language)
  return (
    <div className={cn("flex shrink-0 items-center gap-1.5", className)}>
      <span className="text-[0.6875rem] text-muted-foreground">{t(label)}</span>
      {children}
    </div>
  )
}

type PresetChoice = { key: string; label: string; updates: AudioClipUpdate[] }

/** The steps to the previous and the next preset, as menu choices. */
function steps(
  updates: (direction: "previous" | "next") => AudioClipUpdate[]
): PresetChoice[] {
  return [
    { key: "previous", label: t("Previous preset"), updates: updates("previous") },
    { key: "next", label: t("Next preset"), updates: updates("next") },
  ]
}

/** Halving and doubling, as menu choices. */
function scales(
  updates: (factor: "half" | "double") => AudioClipUpdate[]
): PresetChoice[] {
  return [
    { key: "half", label: t("Halve"), updates: updates("half") },
    { key: "double", label: t("Double"), updates: updates("double") },
  ]
}

/**
 * The presets of one setting, the steps between them and halving and
 * doubling, in a menu beside its control. They are used now and then, and
 * as rows of buttons they took most of the playlist's height. A choice that
 * would change nothing is disabled; one that does is one undo step.
 */
function PresetMenu({
  label,
  groups,
}: {
  label: string
  groups: PresetChoice[][]
}) {
  useLanguageStore((state) => state.language)
  const hint = useHint(label)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={label}
            className="text-muted-foreground"
            {...hint}
          />
        }
      >
        <HugeiconsIcon icon={ArrowDown01Icon} strokeWidth={2} />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-auto min-w-40">
        <DropdownMenuGroup>
          <DropdownMenuLabel>{label}</DropdownMenuLabel>
          {groups.map((group, index) => (
            <Fragment key={group[0]?.key ?? index}>
              {index > 0 && <DropdownMenuSeparator />}
              {group.map((choice) => (
                <DropdownMenuItem
                  key={choice.key}
                  disabled={choice.updates.length === 0}
                  onClick={() => {
                    if (choice.updates.length > 0)
                      void dispatch({
                        type: "updateAudioClips",
                        updates: choice.updates,
                      })
                  }}
                >
                  {choice.label}
                </DropdownMenuItem>
              ))}
            </Fragment>
          ))}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** Picks the mixer track the selected clips play into. */
function RouteSelect({ clips }: { clips: AudioClip[] }) {
  useLanguageStore((state) => state.language)
  const tracks = useProjectStore((state) => state.project.mixer.tracks).filter((track) => !track.current)
  const first = clips[0].content.mixerTrack
  const direct = clips[0].content.output === "direct"
  const mixed = clips.some((clip) => (clip.content.output === "direct") !== direct || (!direct && clip.content.mixerTrack !== first))
  const current = tracks.find((track) => track.id === first)
  const label = mixed ? t("Several routes") : direct ? t("Direct output") : (current?.name ?? t("No track"))
  const hint = useHint(
    t("Choose a mixer track, or Direct output for a finished print that bypasses mixer effects and faders")
  )
  const route = (mixerTrack: TrackId) =>
    void patchSelectedAudioClips({ mixerTrack, output: "mixer" })

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            size="sm"
            aria-label={i18nMessage("Playback route: {value0}", { value0: t(label) })}
            className="w-32 justify-start gap-1 px-1.5"
            {...hint}
          />
        }
      >
        <HugeiconsIcon
          icon={ArrowRight02Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
        {!mixed && !direct && current && (
          <span
            aria-hidden
            className="size-2 shrink-0 rounded-[2px]"
            style={{ backgroundColor: colorToCss(current.color) }}
          />
        )}
        <span className="min-w-0 flex-1 truncate text-left">
          {t(label)}
        </span>
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="max-h-80 w-auto min-w-44">
        <DropdownMenuItem aria-checked={!mixed && direct} onClick={() => void patchSelectedAudioClips({ output: "direct" })}>{t("Direct output")}</DropdownMenuItem>
        <DropdownMenuSeparator />
        {tracks.map((track) => (
          <DropdownMenuItem
            key={track.id}
            aria-checked={!mixed && !direct && track.id === first}
            onClick={() => route(track.id)}
          >
            <span
              aria-hidden
              className="size-2 shrink-0 rounded-[2px]"
              style={{ backgroundColor: colorToCss(track.color) }}
            />
            <span className="truncate">{track.name}</span>
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={() => void routeSelectionToNewTrack()}>{t("New mixer track")}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function Settings({ clips }: { clips: AudioClip[] }) {
  useLanguageStore((state) => state.language)
  const tempoBpm = useProjectStore((state) => state.project.settings.tempoBpm)
  const first = clips[0]
  const { content } = first
  const name = useProjectStore(
    (state) =>
      state.project.samples.find((sample) => sample.id === content.sample)
        ?.name ?? t("Missing sample")
  )
  const longest = Math.max(...clips.map((clip) => clip.length))

  const gain = useClipSetting(content.gain, (value) => ({
    gain: clamp(value, 0, MAX_GAIN),
  }))
  const pan = useClipSetting(content.pan, (value) => ({
    pan: clamp(value, -1, 1),
  }))
  const pitch = useClipSetting(content.pitch, (value) => ({
    pitch: clamp(value, -MAX_TUNE_SEMITONES, MAX_TUNE_SEMITONES),
  }))
  const fadeIn = useClipSetting(content.fadeIn, (value) => ({
    fadeIn: Math.round(clamp(value, 0, longest)),
  }))
  const fadeOut = useClipSetting(content.fadeOut, (value) => ({
    fadeOut: Math.round(clamp(value, 0, longest)),
  }))

  // What the presets, steps and scales of each setting work from.
  const gains = clips.map((clip) => ({ id: clip.id, gain: clip.content.gain }))
  const pans = clips.map((clip) => ({ id: clip.id, pan: clip.content.pan }))
  const pitches = clips.map((clip) => ({ id: clip.id, pitch: clip.content.pitch }))
  const fades = clips.map((clip) => ({
    id: clip.id,
    length: clip.length,
    fadeIn: clip.content.fadeIn,
    fadeOut: clip.content.fadeOut,
  }))

  const many = clips.length > 1
  const title = many ? i18nMessage("{value0} audio clips", { value0: clips.length }) : name
  const speed = speedOf(pitch.value)
  const gainHint = useHint(
    i18nMessage("Clip gain. Drag, or double-click for 0 dB.{value0} The handle at the right end of the clip's title bar does the same", { value0: many ? t(" Changes every selected audio clip") : "" })
  )
  const panHint = useHint(t("Clip pan. Drag, or double-click to center"))
  const pitchHint = useHint(
    i18nMessage("Pitch in semitones. It is a tape speed, not a stretch: {value0} plays at {value1}× speed, so the clip gets {value2}", { value0: formatSemitones(pitch.value, 2), value1: speed.toFixed(2), value2: speed >= 1 ? t("shorter") : t("longer") })
  )
  const reverseHint = useHint(t("Play the audio backwards, from its last frame"))
  const fadeHint = useHint(
    t("Fade length. Drag, or type beats or a time like 250 ms. The handles at the clip's top corners do the same")
  )
  const fadeProps = {
    size: "sm",
    min: 0,
    max: longest,
    scale: FADE_SCALE,
    defaultValue: 0,
    format: (ticks: number) => describeFade(ticks, tempoBpm),
    parse: (text: string) => parseFade(text, tempoBpm),
    showValue: true,
    className:
      "flex-row gap-1.5 [&>[data-slot=knob-value]]:w-24 [&>[data-slot=knob-value]]:font-readout [&>[data-slot=knob-value]]:text-[0.625rem] [&>[data-slot=knob-value]]:whitespace-nowrap [&>[data-slot=knob-value]]:text-muted-foreground",
  } as const

  return (
    <>
      {/* Fixed widths, so the strip wraps the same whatever is selected. */}
      <span className="w-32 shrink-0 truncate font-medium" title={t(title)}>
        {t(title)}
      </span>
      <Labelled label={t("Gain")}>
        <Knob
          size="sm"
          min={0}
          max={MAX_GAIN}
          scale={faderTaper}
          defaultValue={1}
          aria-label={t("Clip gain")}
          showValue
          className="flex-row gap-1.5 [&>[data-slot=knob-value]]:w-14 [&>[data-slot=knob-value]]:font-readout [&>[data-slot=knob-value]]:text-[0.625rem] [&>[data-slot=knob-value]]:whitespace-nowrap"
          {...gainUnit}
          {...gain}
          {...gainHint}
        />
        <PresetMenu
          label={t("Gain presets")}
          groups={[
            GAIN_PRESETS.map(({ label, gain: preset }) => ({
              key: label,
              label: t(label),
              updates: gainPresetUpdates(gains, preset),
            })),
            steps((direction) =>
              clips.flatMap((clip) => {
                const next = nextClipGainPreset(clip.content.gain, direction)
                return next === null ? [] : [{ id: clip.id, patch: { gain: next } }]
              })
            ),
            scales((factor) => gainScaleUpdates(gains, factor)),
          ]}
        />
      </Labelled>
      <ToggleLed
        size="sm"
        pressed={clips.every((clip) => clip.content.normalize === true)}
        aria-label={t("Normalize clip playback")}
        onPressedChange={(normalize) => void patchSelectedAudioClips({ normalize })}
        className="w-auto px-1.5"
      >{t("Normalize")}</ToggleLed>
      <Labelled label={t("Pan")}>
        <PanControl
          size="sm"
          aria-label={t("Clip pan")}
          showValue
          className="flex-row gap-1.5 [&>[data-slot=knob-value]]:w-8 [&>[data-slot=knob-value]]:font-readout [&>[data-slot=knob-value]]:text-[0.625rem]"
          {...pan}
          {...panHint}
        />
        <PresetMenu
          label={t("Pan presets")}
          groups={[
            PAN_PRESETS.map(({ label, pan: preset }) => ({
              key: label,
              label: t(label),
              updates: panPresetUpdates(pans, preset),
            })),
            steps((direction) =>
              clips.flatMap((clip) => {
                const next = nextClipPanPreset(clip.content.pan, direction)
                return next === null ? [] : [{ id: clip.id, patch: { pan: next } }]
              })
            ),
            scales((factor) => clipPanScaleUpdates(pans, factor)),
          ]}
        />
      </Labelled>
      {clips.every((clip) => clip.content.stretch?.mode !== "spectral") && (
        <Labelled label={t("Pitch")}>
          <NumberField
            size="sm"
            aria-label={t("Clip pitch in semitones")}
            min={-MAX_TUNE_SEMITONES}
            max={MAX_TUNE_SEMITONES}
            step={0.01}
            coarseStep={1}
            splitDrag
            defaultValue={0}
            unit="st"
            className="w-20"
            {...pitch}
            {...pitchHint}
          />
          <span
            data-slot="clip-speed"
            className="w-16 font-readout text-[0.625rem] whitespace-nowrap text-muted-foreground"
            title={t("Pitch changes the speed too, like a tape")}
          >
            {i18nMessage("{value0}× speed", { value0: speed.toFixed(2) })}
          </span>
          <PresetMenu
            label={t("Pitch presets")}
            groups={[
              PITCH_PRESETS.map(({ label, pitch: preset }) => ({
                key: label,
                label: t(label),
                updates: pitchPresetUpdates(pitches, preset),
              })),
              steps((direction) =>
                clipPitchPresetStepUpdates(pitches, direction)
              ),
              scales((factor) => clipPitchScaleUpdates(pitches, factor)),
            ]}
          />
        </Labelled>
      )}
      <ClipProcessingControls clips={clips} />
      <ToggleLed
        size="sm"
        pressed={clips.every((clip) => clip.content.reverse)}
        aria-label={t("Reverse")}
        onPressedChange={(reverse) => void patchSelectedAudioClips({ reverse })}
        className="w-auto px-1.5"
        {...reverseHint}
      >{t("Reverse")}</ToggleLed>
      <Labelled label={t("Fade in")}>
        <Knob aria-label={t("Fade in")} {...fadeProps} {...fadeIn} {...fadeHint} />
      </Labelled>
      <Labelled label={t("Fade out")}>
        <Knob aria-label={t("Fade out")} {...fadeProps} {...fadeOut} {...fadeHint} />
        <PresetMenu
          label={t("Fade presets")}
          groups={[
            FADE_PRESETS.map(({ label, fraction }) => ({
              key: label,
              label: t(label),
              updates: fadePresetUpdates(fades, fraction),
            })),
            steps((direction) => clipFadePresetStepUpdates(fades, direction)),
            scales((factor) => fadeScaleUpdates(fades, factor)),
          ]}
        />
      </Labelled>
      <RouteSelect clips={clips} />
    </>
  )
}

/** What the controls show while they only hold the strip's place. */
const PLACEHOLDER: AudioClip = {
  id: 0,
  track: 0,
  start: 0,
  length: 4 * PPQ,
  offset: 0,
  muted: false,
  content: {
    type: "audio",
    sample: 0,
    mixerTrack: 0,
    gain: 1,
    pan: 0,
    fadeIn: 0,
    fadeOut: 0,
    reverse: false,
    pitch: 0,
  },
}
const PLACEHOLDERS = [PLACEHOLDER]

/**
 * The settings of the selected audio clips, in a strip above the timeline.
 * With several clips selected each control changes all of them, in one
 * undo step.
 *
 * The strip is there for as long as the song has an audio clip, selected or
 * not, and is as tall either way: selecting a clip must not move the
 * timeline under the pointer that is pressing it. With nothing selected the
 * controls are laid out unseen and out of reach, and a line says what the
 * strip is for. When the clips it showed go away instead, as an undo of the
 * loop just added takes them, it steps aside until an audio clip is
 * selected again. Double-clicking an audio clip brings back a strip that
 * was hidden with its x.
 */
export function ClipInspector() {
  useLanguageStore((state) => state.language)
  const generation = useProjectGeneration()
  const open = usePlaylistStore((state) => state.inspectorOpen)
  const hasAudio = useProjectStore((state) =>
    state.project.playlist.clips.some((clip) => clip.content.type === "audio")
  )
  return open && hasAudio ? <Strip key={generation} /> : null
}

/**
 * Whether the clips the strip showed went away, rather than being
 * deselected, and nothing is selected since. The strip stays mounted then
 * and is only hidden: an editing dialog of the strip can be waiting for the
 * clips that replace the ones it edits.
 */
function useSelectionVanished(clips: AudioClip[]): boolean {
  const key = clips.map((clip) => clip.id).join(" ")
  const [shown, setShown] = useState(key)
  const [vanished, setVanished] = useState(false)
  if (key !== shown) {
    setShown(key)
    if (key !== "") {
      setVanished(false)
    } else {
      const ids = new Set(
        useProjectStore
          .getState()
          .project.playlist.clips.map((clip) => String(clip.id))
      )
      setVanished(shown.split(" ").every((id) => !ids.has(id)))
    }
  }
  return vanished && key === ""
}

function Strip() {
  useLanguageStore((state) => state.language)
  const clips = useSelectedAudioClips()
  const vanished = useSelectionVanished(clips)
  // An inspector: Delete and Ctrl+D on a clip's setting never delete or
  // copy the clip whose settings these are.
  const scope = useShortcutScope("clipInspector", { keeps: INSPECTOR_KEEPS })
  const idle = clips.length === 0
  const hideHint = useHint(
    t("Hide the clip settings. Double-click an audio clip to show them again")
  )

  return (
    <div
      role="group"
      aria-label={t("Audio clip settings")}
      data-slot="clip-inspector"
      data-idle={idle ? "" : undefined}
      hidden={vanished || undefined}
      {...scope}
      // A narrow window gets a second line, so no setting is out of reach.
      className="relative flex min-h-9 shrink-0 flex-wrap items-center gap-x-2.5 gap-y-1 border-b bg-chassis/40 py-1 pr-1 pl-2 whitespace-nowrap"
    >
      {idle ? (
        <>
          <p className="pointer-events-none absolute inset-y-0 left-2 flex items-center text-muted-foreground">{t("Select an audio clip to edit it")}</p>
          <div aria-hidden inert className="invisible contents">
            <Settings clips={PLACEHOLDERS} />
          </div>
        </>
      ) : (
        <Settings clips={clips} />
      )}
      {/* Editing dialogs belong to the strip, including the interval between
          their replacement event and their Apply reply. */}
      <div
        aria-hidden={idle || undefined}
        inert={idle || undefined}
        className={cn("contents", idle && "invisible")}
      >
        <SliceControls clips={clips} />
        <AnalysisButton />
        <AudioEditorButton
          clip={clips[0]?.id ?? null}
          disabled={clips.length !== 1}
          answersRequests
        />
      </div>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={t("Hide the clip settings")}
        className="ml-auto shrink-0 text-muted-foreground"
        onClick={() => usePlaylistStore.getState().toggleInspector()}
        {...hideHint}
      >
        <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
      </Button>
    </div>
  )
}
