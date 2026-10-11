import { formatMs } from "@/components/audio"

/** Where the image sits between the sides, as percentages from the left. */
export function fieldExtent(width: number, balance = 0) {
  const half = (Math.min(2, Math.max(0, width)) / 2) * 50
  const lean = Math.min(1, Math.max(-1, balance))
  // Balance turns one side down, so the image shrinks away from that side.
  const left = 50 - half * (1 - Math.max(0, lean))
  const right = 50 + half * (1 + Math.min(0, lean))
  return { left, right }
}

/** Which side a phase offset holds back, and by how much, in words. */
export function describeOffset(offsetMs: number): string {
  if (offsetMs === 0) return "Both sides in time"
  const side = offsetMs > 0 ? "Right" : "Left"
  return `${side} side ${formatMs(Math.abs(offsetMs))} late`
}
