import { useRef } from "react"
import type { ActiveNotePose, ChannelId } from "@/bindings"
import { useRealtime } from "@/lib/store/realtime"

export type CharacterPose = { name: string; body: number; leftArm: number; rightArm: number; leftLeg: number; rightLeg: number; lift: number }

export function characterPose(note: Pick<ActiveNotePose, "key" | "velocity"> | null, frame: number): CharacterPose {
  if (!note) return { name: "rest", body: 0, leftArm: 20, rightArm: -20, leftLeg: 0, rightLeg: 0, lift: 0 }
  const energy = Math.min(1, Math.max(0, note.velocity))
  const swing = Math.sin(frame * Math.PI / 12) * energy
  switch (note.key % 4) {
    case 0: return { name: "step", body: 8 * swing, leftArm: 35 * swing, rightArm: -35 * swing, leftLeg: 25 * swing, rightLeg: -25 * swing, lift: 0 }
    case 1: return { name: "wave", body: -8, leftArm: 140 + 15 * swing, rightArm: -20, leftLeg: 0, rightLeg: 10, lift: 0 }
    case 2: return { name: "jump", body: 0, leftArm: 130, rightArm: -130, leftLeg: 30, rightLeg: -30, lift: -18 * Math.abs(swing) }
    default: return { name: "lean", body: 20 * energy, leftArm: 60, rightArm: -60, leftLeg: -15, rightLeg: 15, lift: 0 }
  }
}

/** Moss, an original articulated seed-shaped character. All geometry was
 * newly authored for Windfall; the artwork in this file is dedicated CC0. */
export function NoteCharacter({ channel }: { channel: ChannelId | null }) {
  const root = useRef<SVGGElement>(null)
  const label = useRef<SVGTitleElement>(null)
  useRealtime((frame) => {
    const note = frame.activeNotePoses?.find((note) => note.channel === channel) ?? null
    const number = frame.mediaClock ? Number((BigInt(frame.mediaClock.outputFrame) * 24n / BigInt(frame.mediaClock.sampleRate)) % 48n) : 0
    const pose = characterPose(note, number)
    const element = root.current
    if (!element) return
    element.dataset.pose = pose.name
    element.setAttribute("transform", `translate(100 ${105 + pose.lift}) rotate(${pose.body})`)
    for (const [name, angle] of [["left-arm", pose.leftArm], ["right-arm", pose.rightArm], ["left-leg", pose.leftLeg], ["right-leg", pose.rightLeg]] as const) {
      element.querySelector(`[data-limb="${name}"]`)?.setAttribute("transform", `rotate(${angle})`)
    }
    if (label.current) label.current.textContent = `Moss: ${pose.name}${note ? `, note ${note.key}` : ""}`
  })
  return (
    <svg viewBox="0 0 200 200" role="img" aria-label="Moss note character" className="h-52 w-full text-primary">
      <title ref={label}>Moss: rest</title>
      <ellipse cx="100" cy="181" rx="42" ry="7" fill="currentColor" opacity="0.12" />
      <g ref={root} transform="translate(100 105)" data-pose="rest">
        <g transform="translate(-20 -5)"><path data-limb="left-arm" d="M0 0 Q-24 8 -31 35" fill="none" stroke="currentColor" strokeWidth="9" strokeLinecap="round" /></g>
        <g transform="translate(20 -5)"><path data-limb="right-arm" d="M0 0 Q24 8 31 35" fill="none" stroke="currentColor" strokeWidth="9" strokeLinecap="round" /></g>
        <g transform="translate(-12 29)"><path data-limb="left-leg" d="M0 0 L-3 36 L-17 36" fill="none" stroke="currentColor" strokeWidth="10" strokeLinecap="round" /></g>
        <g transform="translate(12 29)"><path data-limb="right-leg" d="M0 0 L3 36 L17 36" fill="none" stroke="currentColor" strokeWidth="10" strokeLinecap="round" /></g>
        <path d="M0 -49 C-39 -36 -36 27 0 36 C36 27 39 -36 0 -49Z" fill="currentColor" />
        <path d="M0 -43 Q-8 -66 12 -72 Q26 -54 0 -43" fill="currentColor" />
        <circle cx="-11" cy="-13" r="4" fill="var(--background)" /><circle cx="11" cy="-13" r="4" fill="var(--background)" />
        <path d="M-9 3 Q0 12 9 3" fill="none" stroke="var(--background)" strokeWidth="3" strokeLinecap="round" />
      </g>
    </svg>
  )
}
