import { useRef } from "react"

import { useRealtime } from "@/lib/store/realtime"
import { translatedText as t } from "@/lib/i18n"

const MAX_BINS = 64
const HEIGHT = 160
const FLOOR_DB = -90

/** Current power spectrum, drawn directly without storing frames in React. */
export function SpectrumView() {
  const plot = useRef<SVGSVGElement>(null)
  const quiet = useRef<HTMLParagraphElement>(null)

  useRealtime((frame) => {
    const bins = frame.spectrum ?? []
    const count = Math.min(bins.length, MAX_BINS)
    const bars = plot.current?.querySelectorAll("rect")
    if (!bars) return
    let sounding = false
    bars.forEach((bar, index) => {
      const power = bins[index]
      const valid = index < count && Number.isFinite(power) && power >= 0
      const db = valid && power > 0 ? 10 * Math.log10(power) : FLOOR_DB
      const height =
        Math.max(0, Math.min(1, (db - FLOOR_DB) / -FLOOR_DB)) * HEIGHT
      bar.setAttribute("height", String(height))
      bar.setAttribute("y", String(HEIGHT - height))
      bar.setAttribute("x", String((index * MAX_BINS) / (count || 1)))
      bar.setAttribute("width", String((MAX_BINS / (count || 1)) * 0.8))
      bar.style.display = valid ? "" : "none"
      sounding ||= height > 0
    })
    if (quiet.current) {
      quiet.current.hidden = sounding
      quiet.current.style.display = sounding ? "none" : ""
      quiet.current.textContent =
        count === 0 ? t("No spectrum available") : t("Quiet")
    }
  })

  return (
    <section aria-label={t("Spectrum")} className="flex flex-col gap-2">
      <div className="relative rounded-md bg-display p-4">
        <svg
          ref={plot}
          role="img"
          aria-label={t("Current spectrum, low to high frequency")}
          viewBox={`0 0 ${MAX_BINS} ${HEIGHT}`}
          preserveAspectRatio="none"
          className="h-40 w-full text-brand"
        >
          {Array.from({ length: MAX_BINS }, (_, index) => (
            <rect
              key={index}
              x={index}
              y={HEIGHT}
              width={0.8}
              height={0}
              fill="currentColor"
              style={{ display: "none" }}
            />
          ))}
        </svg>
        <p
          ref={quiet}
          className="pointer-events-none absolute inset-0 flex items-center justify-center text-display-dim"
        >
          {t("No spectrum available")}
        </p>
      </div>
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>DC</span>
        <span>{t("Power · −90 to 0 dB")}</span>
        <span>Nyquist</span>
      </div>
    </section>
  )
}
