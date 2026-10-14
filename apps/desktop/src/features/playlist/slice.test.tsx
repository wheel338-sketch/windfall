import { act, fireEvent, render, screen, within } from "@testing-library/react"
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest"

import type { Clip, ClipContent } from "@/bindings"
import { shortcutLabel } from "@/lib/actions"
import { deviceTransform, deviceX } from "@/lib/canvas"
import type { Backend } from "@/lib/ipc"
import { dispatch, redo, undo } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import { setActiveSession } from "./active"
import { cursorFor, intentFor } from "./intents"
import { addClips } from "./ops"
import { attachPointer } from "./pointer"
import {
  at,
  BAR,
  drag,
  FakeSurface,
  history,
  project,
  startPlaylist,
  startSession,
  STEP,
  ui,
} from "./test-utils"
import { PlaylistToolbar } from "./toolbar"
import { historyLabel } from "@/lib/i18n"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let backend: Backend
let started: ReturnType<typeof startSession>
let stop: () => void
let painters: MockInstance<FakeSurface["addOverlayPainter"]>

beforeEach(async () => {
  const app = await startPlaylist()
  backend = app.backend
  painters = vi.spyOn(FakeSurface.prototype, "addOverlayPainter")
  started = startSession()
  const deactivate = setActiveSession(started.session)
  stop = () => {
    deactivate()
    started.stop()
    app.stop()
  }
  ui().setTool("slice")
  ui().setSnap("step")
})

afterEach(() => {
  stop()
  vi.restoreAllMocks()
})

async function contentOf(kind: ClipContent["type"]): Promise<ClipContent> {
  switch (kind) {
    case "pattern":
      return { type: "pattern", pattern: project().patterns[0].id }
    case "audio":
      return {
        type: "audio",
        sample: project().samples[0].id,
        mixerTrack: project().mixer.tracks[0].id,
        gain: 0.7,
        pan: 0.2,
        fadeIn: 60,
        fadeOut: 120,
        reverse: true,
        pitch: 2,
      }
    case "automation": {
      const result = await dispatch({
        type: "addAutomation",
        target: { type: "trackVolume", track: project().mixer.tracks[0].id },
        points: [
          { tick: 0, value: 0.3, curve: 0, hold: false },
          { tick: 2 * BAR, value: 0.8, curve: 0, hold: false },
        ],
      })
      return { type: "automation", automation: result!.created[0] }
    }
  }
}

async function seed(kind: ClipContent["type"] = "pattern") {
  await addClips(
    [
      {
        row: 0,
        start: BAR,
        length: BAR,
        offset: 480,
        muted: true,
        content: await contentOf(kind),
      },
    ],
    "Seed"
  )
  await settle()
  return project().playlist.clips[0]
}

function rightPiece(clip: Clip, tick: number) {
  return {
    track: clip.track,
    start: tick,
    length: clip.start + clip.length - tick,
    offset: clip.offset + tick - clip.start,
    muted: clip.muted,
    content: clip.content,
  }
}

describe("Slice tool", () => {
  it.each(["pattern", "audio", "automation"] as const)(
    "splits a %s clip with the original ID and intact content in one undo step",
    async (kind) => {
      const before = await seed(kind)
      const tick = BAR + 2 * STEP
      const send = vi.spyOn(backend, "dispatch")
      const steps = history().entries.length

      await drag(started.session, at(BAR + STEP, 0), at(tick + 70, 3))
      expect(send).toHaveBeenCalledTimes(1)
      expect(send.mock.calls[0][0]).toEqual({
        type: "batch",
        label: historyLabel("Slice clips", { count: 1 }),
        commands: [
          {
            type: "updateClips",
            updates: [{ id: before.id, patch: { length: 2 * STEP } }],
          },
          { type: "addClips", clips: [rightPiece(before, tick)] },
        ],
      })
      const clips = project().playlist.clips
      expect(clips).toHaveLength(2)
      expect(clips.find((clip) => clip.id === before.id)).toEqual({
        ...before,
        length: 2 * STEP,
      })
      expect(clips.find((clip) => clip.id !== before.id)).toEqual({
        id: expect.any(Number),
        ...rightPiece(before, tick),
      })
      expect(history().entries).toHaveLength(steps + 1)
      expect(started.session.slicePreviewTick).toBeNull()
      await undo()
      expect(project().playlist.clips).toEqual([before])
      await redo()
      expect(project().playlist.clips).toEqual(clips)
    }
  )

  it.each([BAR, 2 * BAR, 3 * BAR])(
    "dispatches nothing at an edge or empty tick %s",
    async (tick) => {
      const before = await seed()
      const send = vi.spyOn(backend, "dispatch")
      const steps = history().entries.length
      await drag(started.session, at(tick, 0), at(tick, 2))
      expect(send).not.toHaveBeenCalled()
      expect(project().playlist.clips).toEqual([before])
      expect(history().entries).toHaveLength(steps)
      expect(started.session.slicePreviewTick).toBeNull()
    }
  )

  it("cuts two crossing clips in one batch without expanding or extending a clip group", async () => {
    const first = await seed()
    await addClips(
      [
        {
          row: 1,
          start: BAR + STEP,
          length: BAR,
          offset: 37,
          muted: false,
          content: first.content,
        },
        {
          row: 2,
          start: 3 * BAR,
          length: BAR,
          offset: 19,
          muted: false,
          content: first.content,
        },
      ],
      "Seed"
    )
    const before = project().playlist.clips
    const second = before[1]
    const outside = before[2]
    await dispatch({ type: "addClipGroup", clips: [first.id, outside.id] })
    await settle()
    const groups = project().playlist.arrangementBook!.clipGroups
    ui().select([outside.id])
    const send = vi.spyOn(backend, "dispatch")
    const steps = history().entries.length
    const tick = BAR + 2 * STEP

    // The line cuts all tracks, even beyond the vertical pointer travel.
    await drag(started.session, at(BAR + STEP, 0), at(tick, 0))
    expect(send).toHaveBeenCalledTimes(1)
    expect(send.mock.calls[0][0]).toEqual({
      type: "batch",
      label: historyLabel("Slice clips", { count: 2 }),
      commands: [
        {
          type: "updateClips",
          updates: [
            { id: first.id, patch: { length: 2 * STEP } },
            { id: second.id, patch: { length: STEP } },
          ],
        },
        {
          type: "addClips",
          clips: [rightPiece(first, tick), rightPiece(second, tick)],
        },
      ],
    })
    expect(project().playlist.clips).toHaveLength(5)
    expect(
      project().playlist.clips.find((clip) => clip.id === outside.id)
    ).toEqual(outside)
    expect(project().playlist.arrangementBook!.clipGroups).toEqual(groups)
    expect([...ui().selection]).toEqual([outside.id])
    expect(history().entries).toHaveLength(steps + 1)
  })

  it("draws a snapped vertical preview without dispatching while held", async () => {
    const before = await seed()
    const send = vi.spyOn(backend, "dispatch")
    const { session, surface } = started
    expect(session.pointerDown(at(BAR + 140, 0))).toBe(true)
    expect(session.slicePreviewTick).toBe(BAR + STEP)
    session.pointerMove(at(BAR + STEP + 140, 2))
    expect(session.slicePreviewTick).toBe(BAR + 2 * STEP)
    expect(project().playlist.clips).toEqual([before])
    expect(send).not.toHaveBeenCalled()

    const paint = painters.mock.calls[1][0]
    const viewport = { ...surface.viewport, scrollTick: BAR, dpr: 2 }
    const transform = deviceTransform(viewport)
    const fillRect = vi.fn()
    const ctx = {
      fillRect,
      fillStyle: "",
    } as unknown as CanvasRenderingContext2D
    paint(ctx, { viewport, transform, theme: surface.theme })
    expect(fillRect).toHaveBeenCalledExactlyOnceWith(
      Math.round(deviceX(transform, BAR + 2 * STEP)),
      0,
      transform.lineWidth,
      transform.heightDev
    )
    session.cancel()
    fillRect.mockClear()
    paint(ctx, { viewport, transform, theme: surface.theme })
    expect(fillRect).not.toHaveBeenCalled()
  })

  it.each(["Alt", "Snap None"])(
    "uses whole ticks with %s, including the release position",
    async (mode) => {
      const before = await seed()
      if (mode === "Snap None") ui().setSnap("none")
      const modifiers = { alt: mode === "Alt" }
      const { session } = started
      session.pointerDown(at(BAR + 300.4, 0, modifiers))
      session.pointerMove(at(BAR + 301.4, 1, modifiers))
      expect(session.slicePreviewTick).toBe(BAR + 301)
      await session.pointerUp(at(BAR + 302.6, 1, modifiers))
      expect(
        project().playlist.clips.find((clip) => clip.id === before.id)?.length
      ).toBe(303)
      expect(
        project().playlist.clips.find((clip) => clip.id !== before.id)
      ).toEqual({
        id: expect.any(Number),
        ...rightPiece(before, BAR + 303),
      })
    }
  )

  it("snaps to the later meter segment's grid origin", async () => {
    const before = await seed()
    await dispatch({
      type: "addMeterChange",
      tick: 4001,
      signature: { numerator: 7, denominator: 8 },
    })
    ui().setSnap("bar")
    await drag(started.session, at(4001, 0), at(4001 + 3360 + 100, 2))
    expect(
      project().playlist.clips.find((clip) => clip.id === before.id)?.length
    ).toBe(7361 - BAR)
  })

  it.each(["Escape", "pointer cancel"])(
    "clears the preview and dispatches nothing after %s",
    async (how) => {
      const before = await seed()
      const send = vi.spyOn(backend, "dispatch")
      const steps = history().entries.length
      const { session, metrics } = started
      const element = document.createElement("div")
      const detach = attachPointer(element, session, metrics, {
        localPoint: (event) => ({ x: event.clientX, y: event.clientY }),
        focus: () => {},
      })
      try {
        const from = at(BAR + STEP, 0)
        const to = at(BAR + 2 * STEP, 1)
        fireEvent.pointerDown(element, {
          clientX: from.x,
          clientY: from.y,
          button: 0,
        })
        fireEvent.pointerMove(element, { clientX: to.x, clientY: to.y })
        expect(session.slicePreviewTick).toBe(BAR + 2 * STEP)
        if (how === "Escape") {
          fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" })
          await settle()
        } else fireEvent.pointerCancel(element)
        expect(session.busy).toBe(false)
        expect(session.slicePreviewTick).toBeNull()
        fireEvent.pointerMove(element, { clientX: to.x, clientY: to.y })
        fireEvent.pointerUp(element, {
          clientX: to.x,
          clientY: to.y,
          button: 0,
        })
        await settle()
        expect(send).not.toHaveBeenCalled()
        expect(project().playlist.clips).toEqual([before])
        expect(history().entries).toHaveLength(steps)
      } finally {
        detach()
      }
    }
  )

  it.each(["windfall", "fl"] as const)(
    "uses C only in the playlist scope in the %s keymap",
    async (keymap) => {
      useUiStore.getState().setKeymap(keymap)
      ui().setTool("draw")
      const other = document.createElement("div")
      other.setAttribute("data-shortcut-scope", "channelRack")
      document.body.append(other)
      try {
        fireEvent.keyDown(other, { key: "c", code: "KeyC" })
        await settle()
        expect(ui().tool).toBe("draw")
      } finally {
        other.remove()
      }
      fireEvent.keyDown(document.body, { key: "c", code: "KeyC" })
      await settle()
      expect(ui().tool).toBe("slice")
      expect(shortcutLabel("playlist.toolSlice")).toBe("C")
    }
  )

  it("selects Slice from the toolbar", async () => {
    ui().setTool("draw")
    render(<PlaylistToolbar metrics={started.metrics} />)
    const tools = within(screen.getByRole("group", { name: "Tools" }))
    const button = tools.getByRole("button", { name: "Slice tool" })
    expect(button).toHaveAttribute("aria-pressed", "false")
    await act(async () => {
      fireEvent.click(button)
      await settle()
    })
    expect(ui().tool).toBe("slice")
    expect(button).toHaveAttribute("aria-pressed", "true")
  })

  it("uses Slice over clip edges and inner handles as well as empty grid", () => {
    for (const part of [null, "body", "start-edge", "end-edge"] as const) {
      const hit = part === null ? null : { id: 1, part }
      expect(
        intentFor({
          tool: "slice",
          button: 0,
          mod: false,
          shift: false,
          hit,
          inner: { kind: "gain" },
        })
      ).toEqual({ kind: "slice" })
      expect(cursorFor("slice", part, { kind: "gain" })).toBe("crosshair")
    }
  })
})
