import { useEffect, useState } from "react"

import { backend } from "@/lib/ipc"
import { useHint } from "@/lib/store/hint"
import { translatedText as t, message } from "@/lib/i18n"

const REFRESH_MS = 1000
const MIB = 1024 * 1024

/** Resident memory of the native host; browser previews have no such process. */
export function MemoryReadout() {
  const [bytes, setBytes] = useState<number | null>(null)
  const hint = useHint(
    t("Host RAM: resident memory of the desktop audio host, excluding webviews and plugin helpers")
  )

  useEffect(() => {
    let active = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const refresh = async () => {
      let next: number | null = null
      try {
        const value = await backend.processMemory()
        if (value !== null && Number.isSafeInteger(value) && value >= 0) {
          next = value
        }
      } catch {
        // A failed OS query is unavailable, and the next poll may recover.
      }
      if (!active) return
      setBytes(next)
      // Schedule after completion: slow native queries never accumulate.
      timer = setTimeout(() => void refresh(), REFRESH_MS)
    }
    void refresh()
    return () => {
      active = false
      clearTimeout(timer)
    }
  }, [])

  const value =
    bytes === null
      ? "—"
      : bytes < 1024 * MIB
        ? `${Math.round(bytes / MIB)} MiB`
        : `${(bytes / (1024 * MIB)).toFixed(1)} GiB`

  return (
    <span
      className="flex items-baseline gap-1"
      aria-label={message("Host RAM: {bytes}; resident memory of the desktop host, excluding webviews and plugin helpers", { bytes: bytes === null ? t("unavailable") : value })}
      title={t("Desktop host resident memory; excludes webviews and plugin helpers")}
      {...hint}
    >
      RAM
      <span className="font-readout text-foreground">{value}</span>
    </span>
  )
}
