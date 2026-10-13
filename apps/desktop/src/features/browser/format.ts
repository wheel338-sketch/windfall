/** A sample's length: "0.42 s", "12.3 s", "1:07". */
import { translatedText as t, message, plainNumber } from "@/lib/i18n"
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0.00 s"
  if (seconds < 10) return `${seconds.toFixed(2)} s`
  if (seconds < 60) return `${seconds.toFixed(1)} s`
  const whole = Math.round(seconds)
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`
}

/** "44.1 kHz", "48 kHz". */
export function formatSampleRate(hz: number): string {
  const khz = hz / 1000
  return `${Number.isInteger(khz) ? khz : khz.toFixed(1)} kHz`
}

export function formatChannels(channels: number): string {
  if (channels === 1) return t("Mono")
  if (channels === 2) return t("Stereo")
  return message("{channels} channels", { channels: plainNumber(channels) })
}
