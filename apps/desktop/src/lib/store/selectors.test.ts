import { renderHook } from "@testing-library/react"
import { afterEach, describe, expect, it } from "vitest"

import type { MixerTrack } from "@/bindings"

import { useProjectStore } from "./project"
import { useMixerTrackIndex, useMixerTrackNumber } from "./selectors"

const before = useProjectStore.getState()
afterEach(() => useProjectStore.setState(before, true))

describe("mixer track numbers", () => {
  it("number the tracks as the strips do, the Current copy aside", () => {
    const [master] = before.project.mixer.tracks
    const track = (id: number, name: string, current = false) => ({ ...master, id, name, current }) as MixerTrack
    const current = track(9_999, "Current", true)
    useProjectStore.setState({
      project: { ...before.project, mixer: { ...before.project.mixer, tracks: [master, current, track(1, "Kick"), track(2, "Clap")] } },
    })
    const target = 1

    expect(renderHook(() => useMixerTrackIndex(target)).result.current).toBe(2)
    expect(renderHook(() => useMixerTrackNumber(target)).result.current).toBe(1)
    expect(renderHook(() => useMixerTrackNumber(master.id)).result.current).toBe(0)
    expect(renderHook(() => useMixerTrackNumber(current.id)).result.current).toBe(-1)
  })
})
