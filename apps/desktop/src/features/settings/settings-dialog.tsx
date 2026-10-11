import { useEffect, type ReactNode } from "react"

import type { AudioSettings, EngineStatus } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { UI_SCALES, supportsUiScale } from "@/lib/ui-scale"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { KEYMAP_PRESETS } from "@/lib/actions/keymap"
import {
  configureEngine,
  loadAudioDevices,
  useEngineStore,
} from "@/lib/store/engine"
import { useUiStore, type KeymapPreset, type Theme } from "@/lib/store/ui"
import { formatSampleRate } from "@/lib/time"
import { cn } from "@/lib/utils"
import { MidiHardwareSettingsSection } from "./midi-hardware"
import { nextKeymapPreset } from "./keymap-preset-step"
import { UpdatesSection } from "@/features/updates/section"
import { LanguageSection } from "./language"
import { useTranslation } from "@/lib/i18n"

import {
  bufferOptions,
  DEFAULT_NUMBER,
  DEFAULT_TEXT,
  fixedBuffer,
  nameOptions,
  sampleRateOptions,
  outputChannelOptions,
  shownDevice,
  withField,
  type Option,
} from "./audio-options"

function Row({ label, children }: { label: string; children: ReactNode }) {
  const t = useTranslation()
  return (
    <div className="grid grid-cols-[7.5rem_1fr] items-center gap-3">
      <span className="text-muted-foreground">{t(label)}</span>
      {children}
    </div>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  const t = useTranslation()
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-xs font-medium">{t(title)}</h3>
      {children}
    </section>
  )
}

function Choice<T extends string | number>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string
  value: T | null
  options: Option<T>[]
  disabled?: boolean
  onChange(value: T): void
}) {
  const t = useTranslation()
  return (
    <Select
      items={options.map((option) => ({ ...option, label: t(option.label) }))}
      value={value}
      disabled={disabled}
      onValueChange={(next: T | null) => {
        if (next !== null) onChange(next)
      }}
    >
      <SelectTrigger aria-label={t(label)} className="w-full">
        <SelectValue placeholder={t("Not available")} />
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false}>
        <SelectGroup>
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {t(option.label)}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

/** What the engine made of the request, in words. */
function runningSummary(status: EngineStatus): string {
  const device = status.device ?? "the default device"
  return `Running on ${device} at ${formatSampleRate(status.sampleRate)} with ${status.outputChannels ?? 2} output channels and a buffer of ${status.bufferFrames} samples: ${status.latencyMs.toFixed(1)} ms of output latency.`
}

/**
 * The audio output. Each box shows what the user asked for, "Default" when
 * they did not; the line below says what the engine is running at. The two
 * differ whenever the device rounds a request or picks its own default, so
 * what the engine reports is never sent back as if the user had chosen it.
 */
function AudioSection() {
  const status = useEngineStore((state) => state.status)
  const hosts = useEngineStore((state) => state.hosts)
  const stored = useEngineStore((state) => state.request)

  useEffect(() => {
    void loadAudioDevices()
  }, [])

  const request = stored ?? {}
  const { host, device } = shownDevice(hosts, request, status)
  const loading = hosts.length === 0 || stored === null
  const fixed = fixedBuffer(device)
  const hasAsio = hosts.some((item) => /asio/i.test(item.name))

  function change<K extends keyof AudioSettings>(
    field: K,
    value: AudioSettings[K] | typeof DEFAULT_TEXT | typeof DEFAULT_NUMBER
  ) {
    let next = withField(request, field, value)
    // Another driver has other devices, so let it pick its default.
    if (field === "host") next = withField(next, "device", DEFAULT_TEXT)
    void configureEngine(next)
  }

  return (
    <Section title="Audio output">
      <Row label="Driver">
        <Choice
          label="Audio driver"
          value={request.host ?? DEFAULT_TEXT}
          disabled={loading}
          options={nameOptions(hosts, request.host)}
          onChange={(name) => change("host", name)}
        />
      </Row>
      <Row label="Device">
        <Choice
          label="Output device"
          value={request.device ?? DEFAULT_TEXT}
          disabled={loading}
          options={nameOptions(host?.devices ?? [], request.device)}
          onChange={(name) => change("device", name)}
        />
      </Row>
      <Row label="Output channels">
        <Choice label="Output channel layout" value={request.outputChannels ?? DEFAULT_NUMBER} disabled={loading || !device} options={outputChannelOptions(device, request.outputChannels)} onChange={(channels) => change("outputChannels", channels)} />
      </Row>
      <Row label="Sample rate">
        <Choice
          label="Sample rate"
          value={request.sampleRate ?? DEFAULT_NUMBER}
          disabled={loading || !device}
          options={sampleRateOptions(device, request.sampleRate)}
          onChange={(sampleRate) => change("sampleRate", sampleRate)}
        />
      </Row>
      <Row label="Buffer size">
        <Choice
          label="Buffer size"
          value={request.bufferFrames ?? DEFAULT_NUMBER}
          disabled={loading || !device}
          options={bufferOptions(
            device,
            request.bufferFrames,
            status?.sampleRate
          )}
          onChange={(bufferFrames) => change("bufferFrames", bufferFrames)}
        />
      </Row>
      {fixed !== null && (
        <p className="text-muted-foreground">
          This driver uses a fixed buffer of {fixed} samples.
          {!hasAsio &&
            " Smaller buffers need an ASIO driver, which this build does not include."}
        </p>
      )}
      <p
        role="status"
        className={cn(
          "rounded-md bg-muted px-2.5 py-1.5 text-muted-foreground",
          status && !status.running && "bg-destructive/10 text-destructive"
        )}
      >
        {!status && "Waiting for the audio engine."}
        {status?.running && runningSummary(status)}
        {status?.running &&
          fixed === null &&
          " A smaller buffer lowers latency and raises CPU load."}
        {status &&
          !status.running &&
          (status.error ?? "The audio output is not running.")}
      </p>
    </Section>
  )
}

const THEMES: Option<Theme>[] = [
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
  { value: "system", label: "Match the system" },
]

function AppearanceSection() {
  const t = useTranslation()
  const theme = useUiStore((state) => state.theme)
  const setTheme = useUiStore((state) => state.setTheme)
  const uiScale = useUiStore((state) => state.uiScale)
  const setUiScale = useUiStore((state) => state.setUiScale)
  const scalable = supportsUiScale()
  return (
    <Section title="Appearance">
      <Row label="Theme">
        <Choice
          label="Theme"
          value={theme}
          options={THEMES}
          onChange={setTheme}
        />
      </Row>
      <FieldGroup>
        <Field>
          <FieldLabel>{t("Interface scale")}</FieldLabel>
          <Choice
            label="Interface scale"
            value={scalable ? uiScale : 100}
            disabled={!scalable}
            options={UI_SCALES.map((value) => ({ value, label: `${value}%` }))}
            onChange={setUiScale}
          />
          <FieldDescription>
            {scalable
              ? t("Scales panels, controls, menus and supported native plugin content. Plugins that refuse scaling keep their current size.")
              : "This webview does not support interface scaling. The interface uses 100%."}
          </FieldDescription>
          <Button variant="outline" size="sm" onClick={() => setUiScale(100)}>
            {t("Restore 100%")}
          </Button>
        </Field>
      </FieldGroup>
    </Section>
  )
}

function KeyboardSection() {
  const keymap = useUiStore((state) => state.keymap)
  const setKeymap = useUiStore((state) => state.setKeymap)
  const preset = KEYMAP_PRESETS.find((item) => item.id === keymap)
  return (
    <Section title="Keyboard">
      <Row label="Shortcuts">
        <Choice<KeymapPreset>
          label="Keyboard shortcuts"
          value={keymap}
          options={KEYMAP_PRESETS.map((item) => ({
            value: item.id,
            label: item.name,
          }))}
          onChange={setKeymap}
        />
      </Row>
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          aria-label="Choose the previous shortcut preset"
          disabled={nextKeymapPreset(keymap, "previous") === null}
          onClick={() => {
            const next = nextKeymapPreset(
              useUiStore.getState().keymap,
              "previous"
            )
            if (next !== null) setKeymap(next as KeymapPreset)
          }}
        >
          Previous
        </Button>
        <Button
          variant="outline"
          size="sm"
          aria-label="Choose the next shortcut preset"
          disabled={nextKeymapPreset(keymap, "next") === null}
          onClick={() => {
            const next = nextKeymapPreset(useUiStore.getState().keymap, "next")
            if (next !== null) setKeymap(next as KeymapPreset)
          }}
        >
          Next
        </Button>
      </div>
      <p className="text-muted-foreground">
        {preset?.about} The command palette lists every shortcut.
      </p>
    </Section>
  )
}

/** Audio device, theme and keyboard shortcuts. */
export function SettingsDialog() {
  const t = useTranslation()
  const open = useUiStore((state) => state.dialog === "settings")
  const closeDialog = useUiStore((state) => state.closeDialog)

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeDialog()
      }}
    >
      <DialogContent className="max-h-[85vh] gap-5 overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("Settings")}</DialogTitle>
          <DialogDescription>
            {t("Changes take effect right away and are kept for next time.")}
          </DialogDescription>
        </DialogHeader>
        <AudioSection />
        <MidiHardwareSettingsSection />
        <AppearanceSection />
        <LanguageSection />
        <KeyboardSection />
        <UpdatesSection />
      </DialogContent>
    </Dialog>
  )
}
