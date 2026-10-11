import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AutomationTarget } from "@/bindings"
import { useRackStore } from "@/features/channel-rack/rack-store"
import { AppShell } from "@/features/layout/app-shell"
import { useEffectsUi } from "@/features/mixer/effects-ui"
import { usePianoRollStore } from "@/features/piano-roll/store"
import { usePlaylistStore } from "@/features/playlist/store"
import { setBackend } from "@/lib/ipc"
import { createMockBackend, type MockBackend } from "@/lib/ipc/mock"
import { useEngineStore } from "@/lib/store/engine"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { useWarningsStore } from "@/lib/store/warnings"
import { settle, TEST_DIALOGS } from "@/test/harness"

/*
 * "Create automation clip" in the menu of every control that can be
 * automated: what each one makes, and that it is one undo step.
 */

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
  Toaster: () => null,
}))

// jsdom lays nothing out, so the real resize handles would take every click.
vi.mock("@/components/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: ReactNode }) => (
    <div className="flex">{children}</div>
  ),
  ResizablePanel: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
  ResizableHandle: () => null,
}))

let backend: MockBackend

beforeEach(async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  backend = createMockBackend({ storage: null, dialogs: TEST_DIALOGS })
  setBackend(backend)
  for (const store of [
    useProjectStore,
    useTransportStore,
    useEngineStore,
    useUiStore,
    usePromptStore,
    useWarningsStore,
    useEffectsUi,
    useRackStore,
    usePianoRollStore,
    usePlaylistStore,
  ]) {
    ;(store as { setState(state: unknown, replace: true): void }).setState(
      store.getInitialState(),
      true
    )
  }
})
afterEach(() => {
  backend.dispose()
  vi.restoreAllMocks()
})

const flush = () => act(settle)
const project = () => useProjectStore.getState().project
const history = () => useProjectStore.getState().history
const labels = () => history().entries.map((entry) => entry.label)

type Ids = {
  kick: number
  synth: number
  kickTrack: number
  compressor: number
  limiter: number
}

/**
 * The whole window: a synth selected in the rack with its settings open,
 * a send from the Kick track, and a compressor and a limiter on the master
 * with their editors open.
 */
async function openWindow(): Promise<Ids> {
  render(<AppShell />)
  await flush()
  const master = project().mixer.tracks[0].id
  const kickTrack = project().mixer.tracks[1].id
  const ids: Ids = {
    kick: project().channels[0].id,
    synth: 0,
    kickTrack,
    compressor: 0,
    limiter: 0,
  }
  await act(async () => {
    const synth = await dispatch({
      type: "addChannel",
      instrument: "subtractiveSynth",
    })
    const compressor = await dispatch({
      type: "addEffect",
      track: master,
      kind: "compressor",
    })
    const limiter = await dispatch({
      type: "addEffect",
      track: master,
      kind: "limiter",
    })
    await dispatch({ type: "setSend", from: kickTrack, to: master, gain: 1 })
    ids.synth = synth!.created[0]
    ids.compressor = compressor!.created[0]
    ids.limiter = limiter!.created[0]
    useUiStore.getState().selectTrack(master)
    useEffectsUi.getState().setInspectorOpen(true)
    useRackStore.getState().setInspectorOpen(true)
    useUiStore.getState().selectChannel(ids.synth)
    await settle()
  })
  return ids
}

const slider = (name: string | RegExp) =>
  screen.getAllByRole("slider", { name })[0]

/** Right-clicks a control and returns its menu. */
async function menuOf(element: Element): Promise<HTMLElement> {
  fireEvent.contextMenu(element, { clientX: 30, clientY: 30 })
  await flush()
  return screen.getByRole("menu")
}

async function closeMenu() {
  const menu = screen.queryByRole("menu")
  if (!menu) return
  fireEvent.keyDown(menu, { key: "Escape" })
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
}

/** Picks an entry of the open menu and lets the edit land. */
async function pick(name: string | RegExp) {
  fireEvent.click(screen.getByRole("menuitem", { name }))
  await flush()
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
  await flush()
}

describe("Create automation clip", () => {
  it(
    "is in the menu of every control that can be automated, and makes the right thing",
    { timeout: 30_000 },
    async () => {
      const ids = await openWindow()
      const master = project().mixer.tracks[0].id
      const cutoff = () => {
        const settings = screen.getByRole("complementary", {
          name: "Channel settings",
        })
        return within(settings).getByRole("slider", { name: "Cutoff" })
      }
      const seams: [
        what: string,
        control: () => Element,
        target: (made: AutomationTarget) => void,
      ][] = [
        [
          "a channel's volume",
          () => slider("Kick channel volume"),
          (made) =>
            expect(made).toEqual({ type: "channelVolume", channel: ids.kick }),
        ],
        [
          "a channel's pan",
          () => slider("Kick channel pan"),
          (made) =>
            expect(made).toEqual({ type: "channelPan", channel: ids.kick }),
        ],
        [
          "a synth's setting",
          cutoff,
          (made) =>
            expect(made).toMatchObject({
              type: "instrumentParam",
              channel: ids.synth,
            }),
        ],
        [
          "a track's fader",
          () => slider("Kick volume"),
          (made) =>
            expect(made).toEqual({ type: "trackVolume", track: ids.kickTrack }),
        ],
        [
          "a track's pan",
          () => slider("Kick pan"),
          (made) =>
            expect(made).toEqual({ type: "trackPan", track: ids.kickTrack }),
        ],
        [
          "a send",
          () => slider("Send to Master"),
          (made) =>
            expect(made).toEqual({
              type: "sendGain",
              track: ids.kickTrack,
              target: master,
            }),
        ],
        [
          "an effect's mix",
          () => slider("Compressor dry/wet mix"),
          (made) =>
            expect(made).toEqual({
              type: "effectMix",
              track: master,
              effect: ids.compressor,
            }),
        ],
        [
          "an effect's setting",
          () => slider("Threshold"),
          (made) =>
            expect(made).toEqual({
              type: "effectParam",
              track: master,
              effect: ids.compressor,
              param: 0,
            }),
        ],
        [
          "the tempo",
          () =>
            screen.getByRole("spinbutton", {
              name: "Tempo in beats per minute",
            }),
          (made) => expect(made).toEqual({ type: "tempo" }),
        ],
      ]

      for (const [what, control, check] of seams) {
        // The controls are in the rack and the mixer; the playlist comes
        // forward each time one is automated.
        await act(async () =>
          useUiStore.getState().showCenterTab("channelRack")
        )
        const before = history().entries.length
        const count = project().automations.length
        const menu = await menuOf(control())
        expect(
          within(menu).getAllByRole("menuitem")[0],
          what
        ).toHaveTextContent("Create automation clip")
        await pick("Create automation clip")

        const automations = project().automations
        expect(automations, what).toHaveLength(count + 1)
        const made = automations.at(-1)!
        check(made.target)
        // One undo step, whatever it took.
        expect(history().entries.length, what).toBe(before + 1)
        expect(labels().at(-1)).toBe("Create automation clip")
        // The new clip is selected on the playlist, which is in view.
        const clip = project().playlist.clips.find(
          (item) =>
            item.content.type === "automation" &&
            item.content.automation === made.id
        )!
        expect(clip, what).toBeDefined()
        expect(useUiStore.getState().centerTab).toBe("playlist")
        expect([...usePlaylistStore.getState().selection]).toEqual([clip.id])
      }

      // The synth's setting is the one the knob edits.
      const synthTarget = project().automations.find(
        (item) => item.target.type === "instrumentParam"
      )!
      expect(synthTarget.name).toMatch(/Cutoff/)

      // Undo takes each of them away again, clip and track and all.
      for (let step = 0; step < seams.length; step += 1) {
        await act(async () => {
          await undo()
        })
      }
      expect(project().automations).toEqual([])
      expect(project().playlist.clips).toEqual([])
      expect(project().playlist.tracks).toEqual([])
    }
  )

  it("hands the keyboard to the timeline, so Delete deletes the clip and not the fader's value", async () => {
    const ids = await openWindow()
    const kickTrack = () =>
      project().mixer.tracks.find((track) => track.id === ids.kickTrack)!
    const fader = () => slider("Kick volume")
    // Somewhere other than its default, so a reset would show.
    act(() => fader().focus())
    fireEvent.keyDown(fader(), { key: "Home" })
    fireEvent.keyUp(fader(), { key: "Home" })
    await flush()
    expect(kickTrack().volume).toBe(0)

    await menuOf(fader())
    await pick("Create automation clip")
    const grid = screen.getByRole("application", { name: "Song timeline" })
    await waitFor(() => expect(grid).toHaveFocus())
    expect(project().playlist.clips).toHaveLength(1)

    fireEvent.keyDown(document.activeElement!, {
      key: "Delete",
      code: "Delete",
    })
    await flush()
    expect(project().playlist.clips).toEqual([])
    expect(labels().at(-1)).toMatch(/^Delete/)
    expect(kickTrack().volume).toBe(0)

    // "Show automation" does the same for a clip that is already there.
    await act(async () => {
      await undo()
    })
    expect(project().playlist.clips).toHaveLength(1)
    await act(async () => {
      useUiStore.getState().showCenterTab("channelRack")
      usePlaylistStore.getState().clearSelection()
    })
    act(() => fader().focus())
    await menuOf(fader())
    await pick("Show automation: Kick track volume")
    await waitFor(() =>
      expect(
        screen.getByRole("application", { name: "Song timeline" })
      ).toHaveFocus()
    )
  })

  it("does not put a focused fader back with Delete or Backspace", async () => {
    const ids = await openWindow()
    const kickTrack = () =>
      project().mixer.tracks.find((track) => track.id === ids.kickTrack)!
    const fader = slider("Kick volume")
    act(() => fader.focus())
    fireEvent.keyDown(fader, { key: "Home" })
    fireEvent.keyUp(fader, { key: "Home" })
    await flush()
    const before = history().entries.length

    fireEvent.keyDown(fader, { key: "Backspace", code: "Backspace" })
    await flush()
    expect(kickTrack().volume).toBe(0)
    expect(history().entries.length).toBe(before)

    // Back to 0 dB with a double-click, or with Ctrl+click.
    fireEvent.doubleClick(fader)
    await flush()
    expect(kickTrack().volume).toBe(1)
  })

  it("is offered by a switch and by a list of choices too", async () => {
    const ids = await openWindow()
    const control = (param: string) =>
      document.querySelector(
        `[data-slot=param-control][data-param="${param}"]`
      )!

    // The compressor's auto make-up is a switch.
    let menu = await menuOf(control("autoMakeup"))
    expect(within(menu).getAllByRole("menuitem")[0]).toHaveTextContent(
      "Create automation clip"
    )
    await pick("Create automation clip")
    let made = project().automations.at(-1)!
    expect(made.target).toMatchObject({
      type: "effectParam",
      effect: ids.compressor,
    })
    expect(made.name).toMatch(/make-?up/i)
    // The switch carries the marker of its automation.
    expect(
      control("autoMakeup").querySelector("[data-slot=param-marker]")
    ).not.toBeNull()

    // The synth's filter mode is a list of three.
    await act(async () => useUiStore.getState().showCenterTab("channelRack"))
    menu = await menuOf(control("filter.mode"))
    expect(within(menu).getAllByRole("menuitem")[0]).toHaveTextContent(
      "Create automation clip"
    )
    await pick("Create automation clip")
    made = project().automations.at(-1)!
    expect(made.target).toMatchObject({
      type: "instrumentParam",
      channel: ids.synth,
    })
    expect(made.name).toMatch(/Filter mode/)
    expect(labels().slice(-2)).toEqual([
      "Create automation clip",
      "Create automation clip",
    ])
  })

  it("is off for the limiter's look-ahead, and says why", async () => {
    await openWindow()
    const menu = await menuOf(slider("Look-ahead"))
    const entry = within(menu).getByRole("menuitem", {
      name: /Create automation clip/,
    })
    expect(entry).toHaveAttribute("aria-disabled", "true")
    expect(entry).toHaveTextContent("Changes the latency")
    await closeMenu()
    // The limiter's other settings can be automated.
    const ceiling = await menuOf(slider("Ceiling"))
    expect(
      within(ceiling).getByRole("menuitem", { name: "Create automation clip" })
    ).not.toHaveAttribute("aria-disabled", "true")
  })

  it("leads back to the automations a control has, and removes them", async () => {
    const ids = await openWindow()
    let menu = await menuOf(slider("Kick volume"))
    expect(within(menu).queryByRole("menuitem", { name: /^Show/ })).toBeNull()
    expect(
      within(menu).queryByRole("menuitem", { name: "Remove automation" })
    ).toBeNull()
    await pick("Create automation clip")
    const [automation] = project().automations
    expect(automation.name).toBe("Kick track volume")

    // The control now carries a marker, in the automation's color.
    const fader = slider("Kick volume")
    const marker = fader.querySelector<HTMLElement>("[data-slot=fader-marker]")
    expect(marker).not.toBeNull()
    expect(marker?.style.backgroundColor).not.toBe("")

    await act(async () => {
      useUiStore.getState().showCenterTab("channelRack")
      usePlaylistStore.getState().clearSelection()
    })
    await menuOf(slider("Kick volume"))
    await pick("Show automation: Kick track volume")
    expect(useUiStore.getState().centerTab).toBe("playlist")
    const clip = project().playlist.clips[0]
    expect([...usePlaylistStore.getState().selection]).toEqual([clip.id])

    // The clip is the one thing between the automation and its end, so
    // removing it asks first.
    menu = await menuOf(slider("Kick volume"))
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "Remove automation" })
    )
    await flush()
    await act(async () => {
      usePromptStore.getState().confirm?.resolve("delete")
      await settle()
    })
    expect(project().automations).toEqual([])
    expect(project().playlist.clips).toEqual([])
    expect(labels().at(-1)).toBe("Delete automation")
    expect(
      slider("Kick volume").querySelector("[data-slot=fader-marker]")
    ).toBeNull()

    // Two automations of one target start with the same name, so the menu
    // numbers them.
    await act(async () => {
      await backend.automate({ type: "trackPan", track: ids.kickTrack })
      await backend.automate({ type: "trackPan", track: ids.kickTrack })
      await settle()
    })
    menu = await menuOf(slider("Kick pan"))
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent)
        .slice(0, 5)
    ).toEqual([
      "Create automation clip",
      "Show automation: Kick track pan",
      "Show automation: Kick track pan 2",
      "Edit events in pattern",
      "Remove automation",
    ])
  })
})
