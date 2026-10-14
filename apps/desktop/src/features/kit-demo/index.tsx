import * as React from "react"
import {
  Moon02Icon,
  PlayIcon,
  StopIcon,
  Sun03Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import {
  EnvelopeEditor,
  Fader,
  Knob,
  LevelMeter,
  MuteSolo,
  NumberField,
  PanControl,
  PianoKeyboard,
  StepGrid,
  StepGridGroup,
  ToggleLed,
  Waveform,
  faderTaper,
  formatGain,
  gainToFaderPosition,
  gainUnit,
  hzUnit,
  msUnit,
  noteName,
  percentUnit,
  powerScale,
  semitonesUnit,
  type EnvelopeValues,
  type LevelMeterHandle,
  type StepGridGroupHandle,
  type WaveformHandle,
} from "@/components/audio"
import { useTheme } from "@/components/theme-provider"
import { Button } from "@/components/ui/button"

import {
  DEMO_CHANNELS,
  makeDemoPeaks,
  panGains,
  type DemoChannel,
} from "./demo-data"
import { KitStates } from "./states"
import { formatNumber, translatedText as t, message } from "@/lib/i18n"

export type KitEvent = { type: string; detail?: unknown }

declare global {
  interface Window {
    /** Every callback the demo received, for scripted checks. */
    __kitLog?: KitEvent[]
  }
}

const STEPS = 16
const MIXER_STRIPS = 5
const TIME_SCALE = powerScale(3)
const ENVELOPE_DEFAULTS: EnvelopeValues = {
  attackMs: 12,
  decayMs: 180,
  sustain: 0.6,
  releaseMs: 320,
}
const CHORD = new Set([60, 64, 67])

function Panel({
  title,
  hint,
  className,
  children,
}: {
  title: string
  hint?: string
  className?: string
  children: React.ReactNode
}) {
  return (
    <section
      className={`flex min-w-0 flex-col rounded-lg border bg-card ${className ?? ""}`}
    >
      <header className="flex h-8 shrink-0 items-baseline gap-3 border-b px-3 pt-2">
        <h2 className="text-xs font-semibold">{title}</h2>
        {hint ? (
          <p className="truncate text-[11px] text-muted-foreground">{hint}</p>
        ) : null}
      </header>
      <div className="min-h-0 flex-1 p-3">{children}</div>
    </section>
  )
}

/** Showcase of the audio control kit, opened with `?view=kit`. */
export default function KitDemo() {
  const { theme, setTheme } = useTheme()
  const isDark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches)

  const [channels, setChannels] = React.useState(DEMO_CHANNELS)
  const [masterGain, setMasterGain] = React.useState(1)
  const [tempo, setTempo] = React.useState(128)
  const [playing, setPlaying] = React.useState(true)
  const [lastEvent, setLastEvent] = React.useState(() => t("Move a control"))

  const [region, setRegion] = React.useState({ start: 0.04, end: 0.82 })
  const [sampler, setSampler] = React.useState({
    tune: 0,
    gain: 1,
    cutoff: 8000,
    mix: 0.35,
  })
  const [envelope, setEnvelope] = React.useState(ENVELOPE_DEFAULTS)
  const [showChord, setShowChord] = React.useState(false)
  const peaks = React.useMemo(() => makeDemoPeaks(), [])

  const rackRef = React.useRef<StepGridGroupHandle>(null)
  const waveformRef = React.useRef<WaveformHandle>(null)
  const stripMeters = React.useRef<(LevelMeterHandle | null)[]>([])
  const masterMeter = React.useRef<LevelMeterHandle>(null)
  const keyboardHit = React.useRef(0)

  const record = React.useCallback(
    (type: string, detail?: unknown, text?: string) => {
      ;(window.__kitLog ??= []).push({ type, detail })
      if (text) {
        setLastEvent(text)
      }
    },
    []
  )

  const gesture = (name: string) => ({
    onGestureStart: () => record("gesture-start", name),
    onGestureEnd: () => record("gesture-end", name),
  })

  const patchChannel = React.useCallback(
    (id: number, patch: Partial<DemoChannel>) => {
      setChannels((current) =>
        current.map((channel) =>
          channel.id === id ? { ...channel, ...patch } : channel
        )
      )
    },
    []
  )

  const toggleStep = React.useCallback(
    (id: number, step: number, on: boolean) => {
      record("step", { channel: id, step, on })
      setChannels((current) =>
        current.map((channel) =>
          channel.id === id
            ? {
                ...channel,
                steps: channel.steps.map((was, index) =>
                  index === step ? on : was
                ),
              }
            : channel
        )
      )
    },
    [record]
  )

  // The stand-in for the engine: it walks the pattern, turns lit steps into
  // decaying hits and feeds the meters, all outside React state.
  const live = React.useRef({ channels, masterGain, tempo, playing, region })
  React.useEffect(() => {
    live.current = { channels, masterGain, tempo, playing, region }
  })
  React.useEffect(() => {
    const energy = new Float32Array(DEMO_CHANNELS.length)
    let frame = 0
    let last = performance.now()
    let position = 0
    let lastStep = -1
    let sweep = -1

    const tick = (now: number) => {
      const seconds = Math.min(0.1, (now - last) / 1000)
      last = now
      const state = live.current
      if (state.playing) {
        position += seconds * (state.tempo / 60) * 4
        const step = Math.floor(position) % STEPS
        if (step !== lastStep) {
          lastStep = step
          rackRef.current?.setPlayStep(step)
          const soloed = state.channels.some((channel) => channel.solo)
          state.channels.forEach((channel, index) => {
            const audible = soloed ? channel.solo : !channel.muted
            if (channel.steps[step] && audible) {
              energy[index] = 1
              if (index === 0) {
                sweep = 0
              }
            }
          })
        }
      } else if (lastStep !== -1) {
        lastStep = -1
        position = 0
        rackRef.current?.setPlayStep(null)
      }

      if (keyboardHit.current > 0) {
        energy[0] = Math.max(energy[0], keyboardHit.current)
        keyboardHit.current = 0
        sweep = 0
      }

      let left = 0
      let right = 0
      state.channels.forEach((channel, index) => {
        energy[index] *= Math.exp(-seconds * channel.decay)
        const level = energy[index] * channel.gain
        const [panLeft, panRight] = panGains(channel.pan)
        stripMeters.current[index]?.set(level * panLeft, level * panRight)
        left += level * panLeft * 0.6
        right += level * panRight * 0.6
      })
      masterMeter.current?.set(
        left * state.masterGain,
        right * state.masterGain
      )

      if (sweep >= 0) {
        sweep += seconds / 0.45
        const { start, end } = state.region
        waveformRef.current?.setPlayhead(
          sweep >= 1 ? null : start + (end - start) * sweep
        )
        if (sweep >= 1) {
          sweep = -1
        }
      }
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [])

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="sticky top-0 z-30 flex h-12 items-center gap-4 border-b bg-background/90 px-4 backdrop-blur">
        <div className="flex items-baseline gap-2">
          <h1 className="text-sm font-semibold tracking-tight">
            {t("Windfall control kit")}
          </h1>
          <p className="text-xs text-muted-foreground">
            {t("Every control the built-in plugins share")}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-2">
          <p
            data-testid="last-event"
            className="mr-2 max-w-72 truncate text-xs text-muted-foreground tabular-nums"
          >
            {lastEvent}
          </p>
          <NumberField
            aria-label={t("Tempo")}
            value={tempo}
            onValueChange={(next) => {
              setTempo(next)
              record("tempo", next, message("Tempo {tempo} BPM", { tempo: formatNumber(next, { minimumFractionDigits: 2, maximumFractionDigits: 2, useGrouping: false }) }))
            }}
            {...gesture("tempo")}
            min={10}
            max={522}
            step={0.01}
            defaultValue={128}
            splitDrag
            unit="BPM"
          />
          <Button
            variant="outline"
            onClick={() => setPlaying((was) => !was)}
            aria-pressed={playing}
          >
            <HugeiconsIcon
              icon={playing ? StopIcon : PlayIcon}
              data-icon="inline-start"
            />
            {playing ? t("Stop") : t("Play")}
          </Button>
          <Button
            variant="outline"
            data-testid="theme-toggle"
            onClick={() => setTheme(isDark ? "light" : "dark")}
          >
            <HugeiconsIcon
              icon={isDark ? Sun03Icon : Moon02Icon}
              data-icon="inline-start"
            />
            {isDark ? t("Light theme") : t("Dark theme")}
          </Button>
        </div>
      </header>

      <main className="mx-auto flex max-w-[1440px] flex-col gap-3 p-3">
        <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-3">
          <Panel
            title={t("Channel rack")}
            hint={t("Click a step to toggle it, drag to paint, right-drag to clear")}
          >
            <StepGridGroup ref={rackRef} data-testid="rack" className="gap-1">
              {channels.map((channel) => (
                <div
                  key={channel.id}
                  data-testid={`rack-row-${channel.id}`}
                  className="flex h-7 items-center gap-2"
                >
                  <ToggleLed
                    variant="dot"
                    size="lg"
                    pressed={!channel.muted}
                    onPressedChange={(on) =>
                      patchChannel(channel.id, { muted: !on })
                    }
                    color="var(--wf-meter-low)"
                    aria-label={message("{channelName} on", { channelName: channel.name })}
                  />
                  <PanControl
                    size="sm"
                    value={channel.pan}
                    onValueChange={(pan) => patchChannel(channel.id, { pan })}
                    aria-label={message("{channelName} pan", { channelName: channel.name })}
                  />
                  <Knob
                    size="sm"
                    value={channel.gain}
                    onValueChange={(gain) => patchChannel(channel.id, { gain })}
                    min={0}
                    max={2}
                    defaultValue={1}
                    scale={faderTaper}
                    {...gainUnit}
                    aria-label={message("{channelName} volume", { channelName: channel.name })}
                  />
                  <span className="w-20 shrink-0 truncate text-xs">
                    {channel.name}
                  </span>
                  <StepGrid
                    steps={channel.steps}
                    color={channel.color}
                    aria-label={message("{channelName} steps", { channelName: channel.name })}
                    onToggle={(step, on) => toggleStep(channel.id, step, on)}
                    {...gesture(`steps-${channel.id}`)}
                  />
                </div>
              ))}
            </StepGridGroup>
          </Panel>

          <Panel title={t("Mixer")} hint={t("Meters follow the pattern")}>
            <div className="flex h-full gap-1">
              <MixerStrip
                name={t("Master")}
                testId="strip-master"
                gain={masterGain}
                onGainChange={(gain) => {
                  setMasterGain(gain)
                  record("gain", gain, message("Master {gain}", { gain: formatGain(gain) }))
                }}
                gesture={gesture("master")}
                meterRef={masterMeter}
              />
              <div className="mx-1 w-px self-stretch bg-border" />
              {channels.slice(0, MIXER_STRIPS).map((channel, index) => (
                <MixerStrip
                  key={channel.id}
                  name={channel.name}
                  testId={`strip-${channel.id}`}
                  gain={channel.gain}
                  onGainChange={(gain) => {
                    patchChannel(channel.id, { gain })
                    record("gain", gain, [channel.name, formatGain(gain)].join(" "))
                  }}
                  gesture={gesture(`strip-${channel.id}`)}
                  pan={channel.pan}
                  onPanChange={(pan) => {
                    patchChannel(channel.id, { pan })
                    record("pan", pan)
                  }}
                  muted={channel.muted}
                  solo={channel.solo}
                  onMutedChange={(muted) => patchChannel(channel.id, { muted })}
                  onSoloChange={(solo) => patchChannel(channel.id, { solo })}
                  meterRef={(handle) => {
                    stripMeters.current[index] = handle
                  }}
                />
              ))}
            </div>
          </Panel>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <Panel
            title={t("Sampler")}
            hint={t("Drag the region handles, or focus one and use the arrow keys")}
          >
            <div className="flex flex-col gap-3">
              <Waveform
                ref={waveformRef}
                data-testid="waveform"
                peaks={peaks}
                start={region.start}
                end={region.end}
                onStartChange={(start) => {
                  setRegion((was) => ({ ...was, start }))
                  record("region-start", start)
                }}
                onEndChange={(end) => {
                  setRegion((was) => ({ ...was, end }))
                  record("region-end", end)
                }}
                {...gesture("region")}
                className="h-24"
              />
              <div className="flex items-start gap-5">
                <Knob
                  label={t("Tune")}
                  showValue
                  bipolar
                  value={sampler.tune}
                  onValueChange={(tune) =>
                    setSampler((was) => ({ ...was, tune }))
                  }
                  min={-24}
                  max={24}
                  step={0.01}
                  keyStep={1}
                  defaultValue={0}
                  format={(value) => semitonesUnit.format(value)}
                  parse={semitonesUnit.parse}
                />
                <Knob
                  label={t("Gain")}
                  showValue
                  data-testid="knob-gain"
                  value={sampler.gain}
                  onValueChange={(gain) => {
                    setSampler((was) => ({ ...was, gain }))
                    record("knob-gain", gain, message("Gain {gain}", { gain: formatGain(gain) }))
                  }}
                  {...gesture("knob-gain")}
                  min={0}
                  max={2}
                  defaultValue={1}
                  {...gainUnit}
                />
                <Knob
                  label={t("Cutoff")}
                  showValue
                  value={sampler.cutoff}
                  onValueChange={(cutoff) =>
                    setSampler((was) => ({ ...was, cutoff }))
                  }
                  min={20}
                  max={20000}
                  scale="log"
                  defaultValue={8000}
                  {...hzUnit}
                />
                <Knob
                  label={t("Mix")}
                  showValue
                  data-testid="knob-mix"
                  value={sampler.mix}
                  onValueChange={(mix) => {
                    setSampler((was) => ({ ...was, mix }))
                    record("knob-mix", mix, message("Mix {mix}", { mix: percentUnit.format(mix) }))
                  }}
                  {...gesture("knob-mix")}
                  defaultValue={0.35}
                  {...percentUnit}
                />
              </div>
            </div>
          </Panel>

          <Panel
            title={t("Envelope")}
            hint={t("The nodes and the knobs edit the same four values")}
          >
            <div className="flex flex-col gap-3">
              <EnvelopeEditor
                data-testid="envelope"
                {...envelope}
                defaults={ENVELOPE_DEFAULTS}
                onChange={(patch) => {
                  setEnvelope((was) => ({ ...was, ...patch }))
                  record("envelope", patch)
                }}
                {...gesture("envelope")}
                className="h-24"
              />
              <div className="flex items-start gap-5">
                <Knob
                  label={t("Attack")}
                  showValue
                  value={envelope.attackMs}
                  onValueChange={(attackMs) =>
                    setEnvelope((was) => ({ ...was, attackMs }))
                  }
                  min={0}
                  max={5000}
                  scale={TIME_SCALE}
                  defaultValue={ENVELOPE_DEFAULTS.attackMs}
                  {...msUnit}
                />
                <Knob
                  label={t("Decay")}
                  showValue
                  value={envelope.decayMs}
                  onValueChange={(decayMs) =>
                    setEnvelope((was) => ({ ...was, decayMs }))
                  }
                  min={0}
                  max={5000}
                  scale={TIME_SCALE}
                  defaultValue={ENVELOPE_DEFAULTS.decayMs}
                  {...msUnit}
                />
                <Knob
                  label={t("Sustain")}
                  showValue
                  value={envelope.sustain}
                  onValueChange={(sustain) =>
                    setEnvelope((was) => ({ ...was, sustain }))
                  }
                  defaultValue={ENVELOPE_DEFAULTS.sustain}
                  {...percentUnit}
                />
                <Knob
                  label={t("Release")}
                  showValue
                  value={envelope.releaseMs}
                  onValueChange={(releaseMs) =>
                    setEnvelope((was) => ({ ...was, releaseMs }))
                  }
                  min={0}
                  max={10000}
                  scale={TIME_SCALE}
                  defaultValue={ENVELOPE_DEFAULTS.releaseMs}
                  {...msUnit}
                />
              </div>
            </div>
          </Panel>
        </div>

        <Panel
          title={t("Keyboard")}
          hint={t("Drag across the keys for a glissando. Lower on a key is louder.")}
        >
          <div className="flex items-center gap-3">
            <PianoKeyboard
              data-testid="keyboard"
              lowKey={36}
              highKey={96}
              activeKeys={showChord ? CHORD : undefined}
              onNoteOn={(key, velocity) => {
                keyboardHit.current = velocity
                record(
                  "note-on",
                  { key, velocity },
                  message("Note on {note} at {velocity}%", { note: noteName(key), velocity: Math.round(velocity * 100) })
                )
              }}
              onNoteOff={(key) =>
                record("note-off", { key }, message("Note off {note}", { note: noteName(key) }))
              }
              className="h-20"
            />
            <label className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
              <ToggleLed
                pressed={showChord}
                onPressedChange={setShowChord}
                aria-label={t("Highlight a C major chord")}
              >
                C
              </ToggleLed>
              {t("Highlight a chord")}
            </label>
          </div>
        </Panel>

        <KitStates />
      </main>
    </div>
  )
}

type MixerStripProps = {
  name: string
  testId: string
  gain: number
  onGainChange: (gain: number) => void
  gesture: { onGestureStart: () => void; onGestureEnd: () => void }
  pan?: number
  onPanChange?: (pan: number) => void
  muted?: boolean
  solo?: boolean
  onMutedChange?: (muted: boolean) => void
  onSoloChange?: (solo: boolean) => void
  meterRef: React.Ref<LevelMeterHandle>
}

function MixerStrip({
  name,
  testId,
  gain,
  onGainChange,
  gesture,
  pan,
  onPanChange,
  muted,
  solo,
  onMutedChange,
  onSoloChange,
  meterRef,
}: MixerStripProps) {
  return (
    <div
      data-testid={testId}
      className="flex w-[76px] flex-col items-center gap-2"
    >
      <span className="w-full truncate text-center text-[11px] font-medium">
        {name}
      </span>
      {pan !== undefined ? (
        <PanControl
          size="sm"
          value={pan}
          onValueChange={onPanChange}
          aria-label={message("{name} pan", { name })}
        />
      ) : (
        <div className="h-6" />
      )}
      <Fader
        value={gain}
        onValueChange={onGainChange}
        {...gesture}
        showValue
        aria-label={message("{name} level", { name })}
        className="h-48"
        meter={<LevelMeter ref={meterRef} taper={gainToFaderPosition} />}
      />
      {muted !== undefined && solo !== undefined ? (
        <MuteSolo
          muted={muted}
          solo={solo}
          onMutedChange={onMutedChange}
          onSoloChange={onSoloChange}
        />
      ) : (
        <div className="h-5" />
      )}
    </div>
  )
}
