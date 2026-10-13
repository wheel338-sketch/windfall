import { useRef } from "react"

import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { useRealtime } from "@/lib/store/realtime"
import { translatedText as t } from "@/lib/i18n"

const MAX_COLUMNS = 64
const MAX_ROWS = 16
const FLOOR_DB = -90

/** Draw the engine's short history directly, without React state per frame. */
export function SpectrogramView() {
  const plot = useRef<SVGSVGElement>(null)
  const empty = useRef<HTMLDivElement>(null)

  useRealtime((frame) => {
    const powers = frame.spectrogram ?? []
    // Current engine snapshots have 64 bands, including when only older
    // slices are valid. A smaller nonempty spectrum supplies its row width.
    const columns = Math.min(frame.spectrum?.length || MAX_COLUMNS, MAX_COLUMNS)
    const complete = powers.length % columns === 0
    const rows = complete ? Math.min(powers.length / columns, MAX_ROWS) : 0
    const offset = Math.max(0, powers.length - rows * columns)
    const cells = plot.current?.querySelectorAll("rect")
    if (!cells) return
    cells.forEach((cell, index) => {
      const power = powers[offset + index]
      const valid =
        index < rows * columns && Number.isFinite(power) && power >= 0
      cell.style.display = valid ? "" : "none"
      if (!valid) return
      const db = power > 0 ? 10 * Math.log10(power) : FLOOR_DB
      const intensity = Math.max(0, Math.min(1, (db - FLOOR_DB) / -FLOOR_DB))
      cell.setAttribute(
        "x",
        String(((index % columns) * MAX_COLUMNS) / columns)
      )
      cell.setAttribute(
        "y",
        String((Math.floor(index / columns) * MAX_ROWS) / rows)
      )
      cell.setAttribute("width", String(MAX_COLUMNS / columns))
      cell.setAttribute("height", String(MAX_ROWS / rows))
      cell.setAttribute("fill-opacity", String(intensity))
    })
    if (empty.current) {
      empty.current.hidden = rows > 0
      empty.current.style.display = rows > 0 ? "none" : ""
    }
  })

  return (
    <section aria-label={t("Spectrogram")} className="flex flex-col gap-2">
      <div className="relative rounded-md bg-display p-4">
        <svg
          ref={plot}
          role="img"
          aria-label={t("Spectrogram, DC to Nyquist from left to right, oldest to newest from top to bottom")}
          viewBox={`0 0 ${MAX_COLUMNS} ${MAX_ROWS}`}
          preserveAspectRatio="none"
          className="h-40 w-full text-brand"
        >
          {Array.from({ length: MAX_ROWS * MAX_COLUMNS }, (_, index) => (
            <rect
              key={index}
              width={1}
              height={1}
              fill="currentColor"
              fillOpacity={0}
              style={{ display: "none" }}
            />
          ))}
        </svg>
        <Empty ref={empty} className="pointer-events-none absolute inset-0">
          <EmptyHeader>
            <EmptyTitle>{t("No spectrogram available")}</EmptyTitle>
          </EmptyHeader>
        </Empty>
      </div>
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>DC</span>
        <span>{t("Power · −90 to 0 dB")}</span>
        <span>Nyquist</span>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("Oldest at top · Newest at bottom")}
      </p>
    </section>
  )
}
