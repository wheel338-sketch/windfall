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

import { useEffectsUi } from "@/features/mixer/effects-ui"
import { useRackStore } from "@/features/channel-rack/rack-store"
import { usePianoRollStore } from "@/features/piano-roll/store"
import { usePlaylistStore } from "@/features/playlist/store"
import { setBackend } from "@/lib/ipc"
import { createMockBackend, type MockBackend } from "@/lib/ipc/mock"
import { useEngineStore } from "@/lib/store/engine"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { useWarningsStore } from "@/lib/store/warnings"
import { settle, TEST_DIALOGS } from "@/test/harness"

import { AppShell } from "./app-shell"

/*
 * The plan's promise: a context menu opens everywhere and lists what can be
 * done to the thing clicked. This walks the window panel by panel, right-
 * clicks one of each kind of thing in it, and expects the app's own menu
 * with something in it that can be done.
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
    // Each store starts as it does in a new window.
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

/** The whole window, with a compressor on the master and its editor open. */
async function openWindow() {
  render(<AppShell />)
  await flush()
  const master = useProjectStore.getState().project.mixer.tracks[0]
  const kick = useProjectStore.getState().project.mixer.tracks[1]
  await act(async () => {
    await dispatch({ type: "addEffect", track: master.id, kind: "compressor" })
    await dispatch({ type: "setSend", from: kick.id, to: master.id, gain: 1 })
    useUiStore.getState().selectTrack(master.id)
    useEffectsUi.getState().setInspectorOpen(true)
    useRackStore.getState().setInspectorOpen(true)
    useUiStore
      .getState()
      .selectChannel(useProjectStore.getState().project.channels[0].id)
    await settle()
  })
}

type Place = [what: string, find: () => Element | null | undefined]

/**
 * Right-clicks each place and expects the app's menu, with at least one
 * entry that is not greyed out. Returns what each menu offered.
 */
async function expectMenus(places: Place[]) {
  const offered = new Map<string, string[]>()
  for (const [what, find] of places) {
    const element = find()
    if (!element) throw new Error(`Nothing to right-click for "${what}"`)
    // Handled by the app, so the webview's own menu stays away.
    const left = fireEvent(
      element,
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: 30,
        clientY: 30,
      })
    )
    await flush()
    expect(left, `${what}: the right-click was not taken`).toBe(false)
    const menu = screen.queryByRole("menu")
    if (!menu) throw new Error(`No menu opened on "${what}"`)
    // Plain entries, on/off entries and the ones that open a submenu.
    const entries = [
      ...menu.querySelectorAll<HTMLElement>("[role^=menuitem]"),
    ].filter((entry) => entry.getAttribute("aria-disabled") !== "true")
    expect(entries.length, `${what}: nothing in the menu can be done`).toBe(
      Math.max(1, entries.length)
    )
    offered.set(
      what,
      entries.map((entry) => entry.textContent ?? "")
    )
    fireEvent.keyDown(menu, { key: "Escape" })
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
  }
  return offered
}

const slot = (name: string) => () =>
  document.querySelector(`[data-slot=${name}]`)
const slider = (name: string | RegExp) => () =>
  screen.getAllByRole("slider", { name })[0]
const tab = (name: string) =>
  act(() => screen.getByRole("tab", { name }).click())

describe("a right-click anywhere opens the app's menu", () => {
  it("on the window's own chrome", { timeout: 20_000 }, async () => {
    await openWindow()
    const offered = await expectMenus([
      ["title bar", () => document.querySelector("header")],
      ["project name", slot("project-title")],
      [
        "transport bar",
        () => screen.getByRole("toolbar", { name: "Transport" }),
      ],
      ["tempo", () => screen.getByRole("spinbutton", { name: /Tempo/ })],
      ["editor tabs", () => screen.getByRole("tablist", { name: "Editors" })],
      ["an editor tab", () => screen.getByRole("tab", { name: "Playlist" })],
      ["status bar", () => document.querySelector("footer")],
      [
        "a panel's title strip",
        () =>
          screen.getByRole("region", { name: "Mixer" }).querySelector("header"),
      ],
    ])
    expect(offered.get("transport bar")).toContain("Play the song")
    expect(offered.get("tempo")?.join("|")).toMatch(/Tap tempo.*Type in value/)
    expect(offered.get("status bar")?.join("|")).toMatch(/Settings…/)
    expect(offered.get("editor tabs")?.join("|")).toMatch(/Reset layout/)
  })

  it("in the browser", { timeout: 20_000 }, async () => {
    await openWindow()
    await waitFor(() => expect(screen.getAllByRole("treeitem").length).toBe(3))
    const offered = await expectMenus([
      ["empty space", slot("browser-scroll")],
      ["panel", slot("browser-panel")],
      ["a folder", () => screen.getAllByRole("treeitem")[0]],
    ])
    expect(offered.get("empty space")?.join("|")).toMatch(
      /Add folder.*Refresh.*Preview sounds/
    )
    // A text field keeps the menu every text field has.
    const filter = screen.getByRole("searchbox", { name: "Filter the browser" })
    expect(fireEvent.contextMenu(filter)).toBe(true)
    expect(screen.queryByRole("menu")).toBeNull()
  })

  it(
    "in the channel rack and a channel's settings",
    { timeout: 20_000 },
    async () => {
      await openWindow()
      const settings = () =>
        screen.getByRole("complementary", { name: "Channel settings" })
      const offered = await expectMenus([
        [
          "toolbar",
          () => screen.getByRole("toolbar", { name: "Channel rack" }),
        ],
        ["empty space", slot("rack-scroll")],
        [
          "a channel's name",
          () => screen.getByRole("button", { name: "Kick" }),
        ],
        ["a channel's volume", slider("Kick channel volume")],
        ["a channel's pan", slider("Kick channel pan")],
        ["swing", slider("Swing")],
        ["pattern length", slider("Pattern length in steps")],
        ["channel settings", settings],
        [
          "a setting",
          () => within(settings()).getByRole("slider", { name: "Tune" }),
        ],
        [
          "an envelope knob",
          () => within(settings()).getByRole("slider", { name: "Gain" }),
        ],
      ])
      expect(offered.get("empty space")).toEqual(
        expect.arrayContaining(["Add channel", "Pattern length"])
      )
      for (const control of ["a channel's volume", "swing", "a setting"]) {
        expect(offered.get(control)).toEqual(
          expect.arrayContaining(["Type in value…Enter", "Copy value"])
        )
      }
      // The bound control offers clip automation and pattern events before
      // the entries about its channel.
      expect(offered.get("a channel's volume")?.slice(0, 3)).toEqual([
        "Create automation clip",
        "Edit events in pattern",
        "Show the mixer track it plays into",
      ])
      expect(offered.get("a channel's pan")?.[0]).toBe("Create automation clip")
    }
  )

  it("in the mixer and an effect's editor", { timeout: 20_000 }, async () => {
    await openWindow()
    const effects = () => screen.getByRole("complementary", { name: "Effects" })
    const offered = await expectMenus([
      ["empty space", slot("mixer")],
      ["a strip's name", slot("strip-header")],
      ["a fader", slider("Master volume")],
      ["a pan knob", slider("Kick pan")],
      ["a send", slider("Send to Master")],
      ["an effect's slot", slot("effect-slot")],
      ["the effects", effects],
      [
        "an effect's header",
        () => effects().querySelector("header + ul header, li header"),
      ],
      ["an effect's mix", slider("Compressor dry/wet mix")],
      ["an effect's setting", slider("Threshold")],
      ["an effect's editor", slot("compressor-editor")],
    ])
    expect(offered.get("empty space")).toEqual(
      expect.arrayContaining(["Add mixer trackAlt+M"])
    )
    // Whatever a control is bound to can be automated, and says so first.
    expect(offered.get("a fader")?.slice(0, 3)).toEqual([
      "Create automation clip",
      "Edit events in pattern",
      "Reset peak readout",
    ])
    expect(offered.get("a send")?.slice(0, 3)).toEqual([
      "Create automation clip",
      "Edit events in pattern",
      "Send level",
    ])
    expect(offered.get("a send")).toContain("Remove send")
    for (const control of ["an effect's mix", "an effect's setting"]) {
      expect(offered.get(control)?.[0]).toBe("Create automation clip")
    }
    expect(offered.get("an effect's setting")).toEqual(
      expect.arrayContaining(["Type in value…Enter", "Copy value"])
    )
    expect(offered.get("an effect's header")).toEqual(
      expect.arrayContaining(["Remove effectDel"])
    )
  })

  it("but not where a right-click already does something", async () => {
    await openWindow()
    // On a step the right button erases, and on a mute lamp it solos.
    const step = document.querySelector("[data-step='0']")
    if (!step) throw new Error("the rack shows no steps")
    fireEvent.contextMenu(step)
    await flush()
    expect(screen.queryByRole("menu")).toBeNull()

    const kick = () => useProjectStore.getState().project.channels[0]
    expect(kick().solo).toBe(false)
    fireEvent.contextMenu(screen.getByRole("button", { name: "Kick on" }))
    await flush()
    expect(screen.queryByRole("menu")).toBeNull()
    expect(kick().solo).toBe(true)
  })

  it("in the piano roll", { timeout: 20_000 }, async () => {
    await openWindow()
    await tab("Piano roll")
    await flush()
    act(() => usePianoRollStore.getState().setTool("select"))
    await expectMenus([
      ["toolbar", () => screen.getByRole("toolbar", { name: "Piano roll" })],
      [
        "note grid",
        () => screen.getByRole("application", { name: "Note grid" }),
      ],
      [
        "ruler",
        () => document.querySelector("canvas[aria-label^='Time ruler']"),
      ],
      ["keyboard", () => document.querySelector("[data-slot=piano-keyboard]")],
      [
        "value lane",
        () =>
          screen.getByRole("application", { name: "Note grid" }).parentElement
            ?.lastElementChild,
      ],
    ])
  })

  it("in the playlist", { timeout: 20_000 }, async () => {
    await openWindow()
    await tab("Playlist")
    await flush()
    act(() => usePlaylistStore.getState().setTool("select"))
    const offered = await expectMenus([
      ["toolbar", () => screen.getByRole("toolbar", { name: "Playlist" })],
      ["panel", slot("playlist")],
      ["a pattern", () => document.querySelector("[data-pattern]")],
      ["a spare track row", () => document.querySelector("[data-spare-row]")],
    ])
    expect(offered.get("toolbar")).toEqual(expect.arrayContaining(["Tool"]))
  })
})
