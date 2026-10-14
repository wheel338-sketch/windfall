import type { AudioDevice, AudioHost, AudioSettings } from "@/bindings"
import type { StoredAudioSettings } from "@/lib/ipc"
import { formatSampleRate } from "@/lib/time"
import { translatedText as t, message } from "@/lib/i18n"

/** The sample rates worth offering. Devices list many more than anyone uses. */
export const COMMON_SAMPLE_RATES = [
  44_100, 48_000, 88_200, 96_000, 176_400, 192_000,
]

const POWERS_OF_TWO = [32, 64, 128, 256, 512, 1024, 2048, 4096]

/** The value of the entry that asks for whatever the system picks. */
export const DEFAULT_TEXT = "default"
export const DEFAULT_NUMBER = 0

export type Option<T> = { value: T; label: string }

/**
 * The host and device the settings are about: the ones asked for, and for
 * anything left at "default" the ones the engine is running on.
 */
export function shownDevice(
  hosts: AudioHost[],
  request: StoredAudioSettings,
  running: { host: string; device: string | null } | null
): { host: AudioHost | undefined; device: AudioDevice | undefined } {
  const hostName = request.host ?? running?.host
  const host =
    hosts.find((item) => item.name === hostName) ??
    hosts.find((item) => item.isDefault)
  const deviceName = request.device ?? running?.device
  const device =
    host?.devices.find((item) => item.name === deviceName) ??
    host?.devices.find((item) => item.isDefault)
  return { host, device }
}

/** The common sample rates the device supports. */
export function sampleRatesFor(device: AudioDevice | undefined): number[] {
  if (!device) return []
  return COMMON_SAMPLE_RATES.filter((rate) => device.sampleRates.includes(rate))
}

/**
 * The buffer sizes the device can be asked for: the powers of two inside
 * the range it reports, and the ends of that range. A driver that does not
 * say gets the usual powers of two.
 */
export function bufferSizesFor(device: AudioDevice | undefined): number[] {
  if (!device) return []
  const min = device.minBufferFrames
  const max = device.maxBufferFrames
  if (min === null || max === null) return POWERS_OF_TWO
  const sizes = POWERS_OF_TWO.filter((size) => size >= min && size <= max)
  return [...new Set([min, ...sizes, max])].sort((a, b) => a - b)
}

/** The one buffer size of a driver that offers no choice, or null. */
export function fixedBuffer(device: AudioDevice | undefined): number | null {
  if (!device || device.minBufferFrames === null) return null
  return device.minBufferFrames === device.maxBufferFrames
    ? device.minBufferFrames
    : null
}

function latencyMs(frames: number, sampleRate: number): string {
  return `${((frames / sampleRate) * 1000).toFixed(1)} ms`
}

/**
 * Options for a number that can be left to the system. A stored value the
 * device does not offer is listed too, so the dialog shows what is saved
 * instead of an empty box. Nothing stored is `null` from the shell and
 * `undefined` in a request made here; both mean there is nothing to add.
 */
function withDefault(
  offered: number[],
  stored: number | null | undefined,
  label: (value: number) => string
): Option<number>[] {
  const options: Option<number>[] = [
    { value: DEFAULT_NUMBER, label: t("Default") },
    ...offered.map((value) => ({ value, label: label(value) })),
  ]
  if (stored != null && !offered.includes(stored)) {
    options.push({
      value: stored,
      label: message("{label}, not offered by this device", { label: label(stored) }),
    })
  }
  return options
}

export function sampleRateOptions(
  device: AudioDevice | undefined,
  stored: number | null | undefined
): Option<number>[] {
  return withDefault(sampleRatesFor(device), stored, formatSampleRate)
}
export function outputChannelOptions(device: AudioDevice | undefined, stored: number | null | undefined): Option<number>[] {
  return withDefault(device?.outputChannels ?? [], stored, (channels) => message("{count} output channels", { count: channels }))
}

/** `sampleRate` is the one in use, to say what each size costs in latency. */
export function bufferOptions(
  device: AudioDevice | undefined,
  stored: number | null | undefined,
  sampleRate: number | undefined
): Option<number>[] {
  return withDefault(bufferSizesFor(device), stored, (frames) =>
    sampleRate
      ? message("{count} samples, {latency}", { count: frames, latency: latencyMs(frames, sampleRate) })
      : message("{count} samples", { count: frames })
  )
}

export function nameOptions(
  items: { name: string }[],
  stored: string | null | undefined
): Option<string>[] {
  const options = [
    { value: DEFAULT_TEXT, label: t("Default") },
    ...items.map((item) => ({ value: item.name, label: item.name })),
  ]
  if (stored != null && !items.some((item) => item.name === stored)) {
    options.push({ value: stored, label: message("{stored}, not found", { stored }) })
  }
  return options
}

/**
 * The request with one field changed, or put back to "default". What comes
 * out holds only the fields that were asked for: one the shell stored as
 * `null` is left out like one that was never there.
 */
export function withField<K extends keyof AudioSettings>(
  request: StoredAudioSettings,
  field: K,
  value: AudioSettings[K] | typeof DEFAULT_TEXT | typeof DEFAULT_NUMBER
): AudioSettings {
  const next: AudioSettings = {}
  if (request.processingThreads != null)
    next.processingThreads = request.processingThreads
  if (request.host != null) next.host = request.host
  if (request.device != null) next.device = request.device
  if (request.sampleRate != null) next.sampleRate = request.sampleRate
  if (request.bufferFrames != null) next.bufferFrames = request.bufferFrames
  if (request.outputChannels != null) next.outputChannels = request.outputChannels
  if (value === DEFAULT_TEXT || value === DEFAULT_NUMBER) delete next[field]
  else next[field] = value as AudioSettings[K]
  return next
}
