import { act, fireEvent, screen } from "@testing-library/react"
import { vi } from "vitest"

import type { MixerTrack } from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { settle } from "@/test/harness"
import { message } from "@/lib/i18n"

/** Helpers the mixer's tests share. Not part of the app. */

export const project = () => useProjectStore.getState().project
export const history = () => useProjectStore.getState().history
export const tracks = () => project().mixer.tracks

export function trackNamed(name: string): MixerTrack {
  const track = tracks().find((item) => item.name === name)
  if (!track) throw new Error(message("There is no track called \"{name}\"", { name }))
  return track
}

export function channelNamed(name: string) {
  const channel = project().channels.find((item) => item.name === name)
  if (!channel) throw new Error(message("There is no channel called \"{name}\"", { name }))
  return channel
}

/** The strip of the track with this name. */
export function strip(name: string): HTMLElement {
  return screen.getByRole("group", { name: new RegExp(`^${name}, `) })
}

/** Lets dispatched edits come back from the mock backend and render. */
export async function flush() {
  await act(async () => {
    await settle()
  })
}

/**
 * Drags a kit control with the pointer. jsdom lays nothing out, so a fader
 * is told how tall it is: `travel` pixels move it over its whole range.
 * Each step is a pointer move of that many pixels, up for positive ones.
 */
export function drag(slider: HTMLElement, steps: number[], travel = 100) {
  Object.defineProperty(slider, "clientHeight", {
    configurable: true,
    value: travel + 14,
  })
  // Without a pointer type the kit takes two presses in a row for a double
  // tap, which resets the control.
  const pointer = { pointerId: 1, pointerType: "mouse" }
  let y = 500
  fireEvent.pointerDown(slider, { ...pointer, button: 0, clientY: y })
  for (const step of steps) {
    y -= step
    fireEvent.pointerMove(slider, { ...pointer, clientY: y })
  }
  fireEvent.pointerUp(slider, pointer)
}

/** Gives the strip scroller a size, which jsdom would report as 0. */
export function sizeMixer(width: number, height: number) {
  const isScroller = (element: Element) =>
    element instanceof HTMLElement && element.dataset.slot === "mixer-dock-middle"
  vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(
    function (this: Element) {
      return isScroller(this) ? width : 0
    }
  )
  vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(
    function (this: Element) {
      return isScroller(this) ? height : 0
    }
  )
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(
    function (this: HTMLElement) {
      return isScroller(this) ? height : 0
    }
  )
}

/** jsdom has no canvas; the meters draw nothing in tests. */
export function stubCanvas() {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
}
