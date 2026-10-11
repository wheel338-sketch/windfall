import { useRef } from "react"
import { useRealtime } from "@/lib/store/realtime"

/** Engine-wide overflow count; updated without rendering the rack at 60 Hz. */
export function EchoStatus() {
  const output = useRef<HTMLOutputElement>(null)
  useRealtime((frame) => {
    if (output.current)
      output.current.textContent = `${frame.rejectedEchoes ?? 0} notes had repeats refused (engine total)`
  })
  return <output ref={output} aria-label="Refused note echoes" className="block px-2.5 text-xs text-muted-foreground">0 notes had repeats refused (engine total)</output>
}
