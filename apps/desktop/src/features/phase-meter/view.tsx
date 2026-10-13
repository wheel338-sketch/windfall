import { useRef } from "react"

import { useRealtime } from "@/lib/store/realtime"
import { translatedText as t } from "@/lib/i18n"

/** Existing stereo correlation, painted without a React render per frame. */
export function PhaseMeterView() {
  const meter = useRef<HTMLDivElement>(null)
  const indicator = useRef<HTMLSpanElement>(null)
  const readout = useRef<HTMLParagraphElement>(null)

  useRealtime((frame) => {
    const value = frame.correlation
    const valid =
      value !== undefined && Number.isFinite(value) && value >= -1 && value <= 1
    if (!meter.current || !indicator.current || !readout.current) return

    indicator.current.hidden = !valid
    meter.current.setAttribute(
      "aria-valuetext",
      valid ? value.toFixed(2) : t("No correlation available")
    )
    if (valid) {
      indicator.current.style.left = `${((value + 1) / 2) * 100}%`
      meter.current.setAttribute("aria-valuenow", String(value))
      readout.current.textContent = value.toFixed(2)
    } else {
      meter.current.removeAttribute("aria-valuenow")
      readout.current.textContent = t("No correlation available")
    }
  })

  return (
    <section aria-label={t("Phase meter")} className="flex flex-col gap-2">
      <div className="rounded-md bg-display p-4">
        <div
          ref={meter}
          role="meter"
          aria-label={t("Stereo correlation")}
          aria-valuemin={-1}
          aria-valuemax={1}
          aria-valuetext={t("No correlation available")}
          className="relative h-4 rounded-sm bg-display-dim/20"
        >
          <span
            aria-hidden="true"
            className="absolute inset-y-0 left-1/2 w-px bg-display-dim"
          />
          <span
            ref={indicator}
            hidden
            aria-hidden="true"
            className="absolute inset-y-0 w-1 -translate-x-1/2 rounded-sm bg-brand"
          />
        </div>
        <div className="mt-2 flex justify-between text-xs text-display-dim">
          <span>{t("Side · −1")}</span>
          <span>0</span>
          <span>{t("Mid · +1")}</span>
        </div>
      </div>
      <p
        ref={readout}
        className="text-center font-mono text-xs text-muted-foreground"
      >
        {t("No correlation available")}
      </p>
    </section>
  )
}
