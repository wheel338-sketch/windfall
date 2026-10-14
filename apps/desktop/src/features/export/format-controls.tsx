import type { BitDepth, ExportFormat, ExportOptions } from "@/bindings"
import { useEffect, useState } from "react"
import { backend, errorMessage } from "@/lib/ipc"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from "@/components/ui/field"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { formatSampleRate } from "@/lib/time"

import { Choice, type Option } from "./choice"
import { localizeBackendText, message, plainNumber, translatedText as t } from "@/lib/i18n"

type Settings = Pick<
  ExportOptions,
  "format" | "bitDepth" | "sampleRate" | "flacLevel" | "oggQuality" | "mp3" | "aacBitrate"
>

const FORMATS: Option<ExportFormat>[] = [
  { value: "wav", label: "WAV" },
  { value: "flac", label: "FLAC" },
  { value: "ogg", label: "OGG" },
  { value: "mp3", label: "MP3" },
  { value: "wv", label: "WavPack" },
  { value: "m4a", label: "M4A (AAC)" },
]
const DEPTHS: Option<BitDepth>[] = [
  { value: "int16", get label() { return t("16 bit") } },
  { value: "int24", get label() { return t("24 bit") } },
  { value: "float32", get label() { return t("32 bit float") } },
]
const RATES = [44_100, 48_000, 88_200, 96_000]
const MP3_DEFAULT: NonNullable<ExportOptions["mp3"]> = {
  rate: { mode: "cbr", bitrate: 192 },
  channels: "jointStereo",
}

/** The form offers rates every bitrate of its selected format supports. */
export function exportSampleRates(format: ExportFormat): number[] {
  return format === "mp3" || format === "m4a" ? [44_100, 48_000] : RATES
}

/** Keep the choices valid when changing from a lossless to a lossy format. */
export function changeExportFormat<T extends Settings>(
  draft: T,
  format: ExportFormat
): Omit<T, "format" | "bitDepth" | "sampleRate"> &
  Pick<Settings, "format" | "bitDepth" | "sampleRate"> {
  return {
    ...draft,
    format,
    bitDepth:
      (format === "flac" || format === "wv") && draft.bitDepth === "float32"
        ? "int24"
        : draft.bitDepth,
    sampleRate: exportSampleRates(format).includes(draft.sampleRate)
      ? draft.sampleRate
      : 48_000,
  }
}

export function FormatControls({
  settings,
  disabled,
  set,
  onFormat,
}: {
  settings: Settings
  disabled: boolean
  set<K extends keyof Settings>(key: K, value: Settings[K]): void
  onFormat(format: ExportFormat): void
}) {
  const [m4aReason, setM4aReason] = useState<string | null>(() => t("Checking the Windows AAC encoder…"))
  useEffect(() => {
    let active = true
    void backend.m4aCapability().then(reason => { if (active) setM4aReason(reason && localizeBackendText(reason)) })
      .catch(error => { if (active) setM4aReason(errorMessage(error)) })
    return () => { active = false }
  }, [])
  const mp3 = settings.mp3 ?? MP3_DEFAULT
  const lossless = settings.format === "wav" || settings.format === "flac" || settings.format === "wv"
  return (
    <FieldGroup>
      <Field data-disabled={disabled || undefined}>
        <FieldTitle id="export-format-label">{t("File format")}</FieldTitle>
        <ToggleGroup
          aria-labelledby="export-format-label"
          value={[settings.format]}
          variant="outline"
          disabled={disabled}
          onValueChange={(values) => {
            const format = FORMATS.find(
              (option) => option.value === values[0]
            )?.value
            if (format) onFormat(format)
          }}
        >
          {FORMATS.map(({ value, label }) => (
            <ToggleGroupItem key={value} value={value} disabled={disabled || (value === "m4a" && m4aReason !== null)} title={value === "m4a" ? m4aReason ?? undefined : undefined}>
              {label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        {m4aReason && <FieldDescription>M4A: {m4aReason}</FieldDescription>}
        <FieldDescription>
          {settings.format === "wav"
            ? t("Uncompressed audio for editing and sharing with other music apps.")
            : settings.format === "flac" || settings.format === "wv"
              ? t("Lossless audio in a smaller file.")
              : settings.format === "ogg"
                ? t("Compressed Vorbis audio, with a choice of quality.")
                : t("Compressed audio for players and sharing.")}
        </FieldDescription>
      </Field>
      <FieldGroup className="grid grid-cols-2 gap-3">
        {lossless && (
          <Field data-disabled={disabled || undefined}>
            <FieldLabel htmlFor="export-depth">{t("Bit depth")}</FieldLabel>
            <Choice
              id="export-depth"
              value={settings.bitDepth}
              options={
                settings.format === "flac" || settings.format === "wv"
                  ? DEPTHS.filter((option) => option.value !== "float32")
                  : DEPTHS
              }
              disabled={disabled}
              onChange={(depth) => set("bitDepth", depth)}
            />
          </Field>
        )}
        <Field data-disabled={disabled || undefined}>
          <FieldLabel htmlFor="export-rate">{t("Sample rate")}</FieldLabel>
          <Choice
            id="export-rate"
            value={settings.sampleRate}
            options={exportSampleRates(settings.format).map((value) => ({
              value,
              label: formatSampleRate(value),
            }))}
            disabled={disabled}
            onChange={(rate) => set("sampleRate", rate)}
          />
        </Field>
        {settings.format === "flac" && (
          <Field data-disabled={disabled || undefined}>
            <FieldLabel htmlFor="export-flac-level">{t("Compression")}</FieldLabel>
            <Choice
              id="export-flac-level"
              value={settings.flacLevel ?? 5}
              options={[
                { value: 0, label: t("Fastest") },
                { value: 5, label: t("Standard") },
                { value: 8, label: t("Smallest file") },
              ]}
              disabled={disabled}
              onChange={(value) => set("flacLevel", value)}
            />
          </Field>
        )}
        {settings.format === "m4a" && (
          <Field data-disabled={disabled || undefined}>
            <FieldLabel htmlFor="export-aac-bitrate">{t("AAC bitrate")}</FieldLabel>
            <Choice id="export-aac-bitrate" value={settings.aacBitrate ?? 192}
              options={[96, 128, 160, 192].map(value => ({value, label: `${value} kbps`}))}
              disabled={disabled} onChange={value => set("aacBitrate", value)} />
          </Field>
        )}
        {settings.format === "ogg" && (
          <Field data-disabled={disabled || undefined}>
            <FieldLabel htmlFor="export-ogg-quality">{t("Vorbis quality")}</FieldLabel>
            <Choice
              id="export-ogg-quality"
              value={settings.oggQuality ?? 6}
              options={Array.from({ length: 12 }, (_, index) => ({
                value: index - 1,
                label: index === 0 ? message("{value} (smallest)", { value: plainNumber(index - 1) }) : index === 11 ? message("{value} (highest)", { value: plainNumber(index - 1) }) : String(index - 1),
              }))}
              disabled={disabled}
              onChange={(value) => set("oggQuality", value)}
            />
          </Field>
        )}
        {settings.format === "mp3" && (
          <>
            <Field data-disabled={disabled || undefined}>
              <FieldLabel htmlFor="export-mp3-mode">{t("Bitrate mode")}</FieldLabel>
              <Choice
                id="export-mp3-mode"
                value={mp3.rate.mode}
                options={[
                  { value: "cbr", label: t("Constant bitrate") },
                  { value: "vbr", label: t("Variable bitrate") },
                ]}
                disabled={disabled}
                onChange={(mode) =>
                  set("mp3", {
                    ...mp3,
                    rate:
                      mode === "cbr"
                        ? { mode, bitrate: 192 }
                        : { mode, quality: 2 },
                  })
                }
              />
            </Field>
            <Field data-disabled={disabled || undefined}>
              <FieldLabel htmlFor="export-mp3-rate">
                {mp3.rate.mode === "cbr" ? t("Bitrate") : t("MP3 quality")}
              </FieldLabel>
              {mp3.rate.mode === "cbr" ? (
                <Choice
                  id="export-mp3-rate"
                  value={mp3.rate.bitrate}
                  options={[128, 192, 256, 320].map((value) => ({
                    value,
                    label: `${value} kbps`,
                  }))}
                  disabled={disabled}
                  onChange={(bitrate) =>
                    set("mp3", { ...mp3, rate: { mode: "cbr", bitrate } })
                  }
                />
              ) : (
                <Choice
                  id="export-mp3-rate"
                  value={mp3.rate.quality}
                  options={Array.from({ length: 10 }, (_, value) => ({
                    value,
                    label: value === 0 ? message("{value} (highest)", { value: plainNumber(value) }) : value === 9 ? message("{value} (smallest)", { value: plainNumber(value) }) : String(value),
                  }))}
                  disabled={disabled}
                  onChange={(quality) =>
                    set("mp3", { ...mp3, rate: { mode: "vbr", quality } })
                  }
                />
              )}
            </Field>
            <Field data-disabled={disabled || undefined}>
              <FieldLabel htmlFor="export-mp3-channels">{t("Channels")}</FieldLabel>
              <Choice
                id="export-mp3-channels"
                value={mp3.channels}
                options={[
                  { value: "jointStereo", label: t("Joint stereo") },
                  { value: "stereo", label: t("Stereo") },
                  { value: "mono", label: t("Mono") },
                ]}
                disabled={disabled}
                onChange={(channels) => set("mp3", { ...mp3, channels })}
              />
            </Field>
          </>
        )}
      </FieldGroup>
    </FieldGroup>
  )
}
