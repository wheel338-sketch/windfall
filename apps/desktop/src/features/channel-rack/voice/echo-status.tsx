import { useRef } from "react"
import { useRealtime } from "@/lib/store/realtime"
import { message, plainNumber, translatedText as t } from "@/lib/i18n"

/** Engine-wide overflow count; updated without rendering the rack at 60 Hz. */
export function EchoStatus() {
  const output = useRef<HTMLOutputElement>(null)
  useRealtime((frame) => {
    if (output.current)
      output.current.textContent = message("{rejectedEchoes} notes had repeats refused (engine total)", { rejectedEchoes: plainNumber(frame.rejectedEchoes ?? 0) })
  })
  return <output ref={output} aria-label={t("Refused note echoes")} className="block px-2.5 text-xs text-muted-foreground">{t("0 notes had repeats refused (engine total)")}</output>
}
