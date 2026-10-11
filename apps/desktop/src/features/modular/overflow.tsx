import { useRef } from "react"
import { useRealtime } from "@/lib/store"

/** Ref-backed realtime output; callback observations stay outside React state. */
export function RackOverflow() {
  const output = useRef<HTMLOutputElement>(null)
  useRealtime((frame) => {
    if (!output.current) return
    const count = frame.rejectedRackEvents ?? 0
    const text = count > 0 ? `Rack events refused: ${count} since engine start. Reduce simultaneous notes or graph fanout.` : ""
    if (output.current.textContent !== text) output.current.textContent = text
  })
  return <output ref={output} aria-label="Rack event refusals" aria-live="polite" className="text-sm text-destructive" />
}
