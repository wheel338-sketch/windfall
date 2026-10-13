import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { afterEach, describe, expect, it, vi } from "vitest"

import {
  dbToGain,
  Fader,
  formatGain,
  gainUnit,
  hzUnit,
  Knob,
  NumberField,
  PanControl,
  PLAIN_NUMBER,
} from "@/components/audio"

import {
  clearCopiedValue,
  pasteOutcome,
  ValueContextItems,
  ValueContextMenus,
} from "./value-context-menu"

type Log = string[]

function LiveKnob({
  name,
  start,
  log = [],
  disabled,
}: {
  name: string
  start: number
  log?: Log
  disabled?: boolean
}) {
  const [value, setValue] = useState(start)
  return (
    <Knob
      label={name}
      min={0}
      max={10}
      step={0.5}
      defaultValue={2}
      disabled={disabled}
      value={value}
      onGestureStart={() => log.push("start")}
      onValueChange={(next) => {
        log.push(`change ${next}`)
        setValue(next)
      }}
      onGestureEnd={() => log.push("end")}
    />
  )
}

const slider = (name: string) => screen.getByRole("slider", { name })
const rootOf = (name: string) => {
  const root = slider(name).closest<HTMLElement>(
    "[data-slot=knob], [data-slot=pan-control], [data-slot=fader], [data-slot=number-field]"
  )
  if (!root) throw new Error(`"${name}" is not a kit control`)
  return root
}
const menu = () => screen.getByRole("menu")
const item = (name: string | RegExp) =>
  within(menu()).getByRole("menuitem", { name })

it("pastes a retained localized bare-number readout using its canonical numeric input", () => {
  const parse = vi.fn((text: string) => Number(text))
  expect(pasteOutcome({ unitKind: PLAIN_NUMBER, parse }, {
    value: 1.25, unitKind: PLAIN_NUMBER, text: "1,25", canonicalText: "1.25",
  })).toEqual({ value: 1.25 })
  expect(parse).toHaveBeenCalledWith("1.25")
})

/** A right-click, which opens the menu a moment later. */
async function rightClick(element: HTMLElement) {
  const notPrevented = fireEvent.contextMenu(element, {
    clientX: 40,
    clientY: 60,
  })
  await act(async () => {
    await Promise.resolve()
  })
  return notPrevented
}

afterEach(() => {
  vi.restoreAllMocks()
  clearCopiedValue()
})

describe("the right-click menu of a value control", () => {
  it("opens on a knob and is headed by what was clicked", async () => {
    render(
      <ValueContextMenus>
        <LiveKnob name="Cutoff" start={5} />
      </ValueContextMenus>
    )
    // The webview's own menu is kept away.
    expect(await rightClick(rootOf("Cutoff"))).toBe(false)
    expect(menu()).toHaveTextContent("Cutoff: 5.0")
    expect(
      within(menu())
        .getAllByRole("menuitem")
        .map((entry) => entry.textContent)
    ).toEqual([
      "Reset to defaultCtrl+click",
      "Type in value…Enter",
      "Copy value",
      "Paste value",
    ])
    // Nothing was copied yet.
    expect(item(/^Paste value/)).toHaveAttribute("aria-disabled", "true")
  })

  it("puts the control back to its default as one gesture", async () => {
    const user = userEvent.setup()
    const log: Log = []
    render(
      <ValueContextMenus>
        <LiveKnob name="Cutoff" start={5} log={log} />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Cutoff"))
    await user.click(item(/^Reset to default/))
    expect(slider("Cutoff")).toHaveAttribute("aria-valuenow", "2")
    expect(log).toEqual(["start", "change 2", "end"])

    // At its default there is nothing to put back.
    await rightClick(rootOf("Cutoff"))
    expect(item(/^Reset to default/)).toHaveAttribute("aria-disabled", "true")
  })

  it("copies a value from one control and pastes it into another", async () => {
    const user = userEvent.setup()
    const log: Log = []
    render(
      <ValueContextMenus>
        <LiveKnob name="Cutoff" start={7.5} />
        <LiveKnob name="Resonance" start={1} log={log} />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Cutoff"))
    await user.click(item("Copy value"))

    await rightClick(rootOf("Resonance"))
    expect(menu()).toHaveTextContent("Resonance: 1.0")
    await user.click(item("Paste value (7.5)"))
    expect(slider("Resonance")).toHaveAttribute("aria-valuenow", "7.5")
    expect(log).toEqual(["start", "change 7.5", "end"])
  })

  it("does not paste what the control cannot read", async () => {
    const user = userEvent.setup()
    function Gain() {
      const [gain, setGain] = useState(1)
      return <Fader aria-label="Volume" value={gain} onValueChange={setGain} />
    }
    render(
      <ValueContextMenus>
        <Gain />
        <PanControl aria-label="Pan" value={0} />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Pan"))
    expect(menu()).toHaveTextContent("Pan: C")
    await user.click(item("Copy value"))
    await rightClick(rootOf("Volume"))
    expect(menu()).toHaveTextContent(`Volume: ${formatGain(1)}`)
    // "C" is the middle of the stereo field, not a level.
    expect(item(/^Paste value \(C\)/)).toHaveAttribute("aria-disabled", "true")
    expect(item(/^Paste value \(C\)/)).toHaveTextContent("Another unit")
  })

  it("does not paste a level into a pan, whatever its digits would mean there", async () => {
    const user = userEvent.setup()
    function Controls() {
      const [gain, setGain] = useState(dbToGain(-7.5))
      const [other, setOther] = useState(1)
      const [pan, setPan] = useState(0)
      return (
        <>
          <Fader aria-label="Volume" value={gain} onValueChange={setGain} />
          <Knob
            aria-label="Send"
            min={0}
            max={2}
            value={other}
            onValueChange={setOther}
            {...gainUnit}
          />
          <PanControl aria-label="Pan" value={pan} onValueChange={setPan} />
        </>
      )
    }
    render(
      <ValueContextMenus>
        <Controls />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Volume"))
    await user.click(item("Copy value"))

    // "−7.5 dB" read as a pan would be L8. It is a level, so it stays out.
    await rightClick(rootOf("Pan"))
    const paste = item(/^Paste value \(−7\.5 dB\)/)
    expect(paste).toHaveAttribute("aria-disabled", "true")
    expect(paste).toHaveTextContent("Another unit")
    fireEvent.click(paste)
    fireEvent.keyDown(menu(), { key: "Escape" })
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    expect(slider("Pan")).toHaveAttribute("aria-valuenow", "0")

    // Another level takes the number itself, not its rounded readout.
    await rightClick(rootOf("Send"))
    const same = item(/^Paste value \(−7\.5 dB\)/)
    expect(same).not.toHaveAttribute("aria-disabled", "true")
    await user.click(same)
    expect(Number(slider("Send").getAttribute("aria-valuenow"))).toBeCloseTo(
      dbToGain(-7.5),
      12
    )
  })

  it("decides by the kind of unit a value is in", () => {
    const control = (unitKind: string | undefined) => ({
      unitKind,
      // Reads a typed number, as a control's text entry would.
      parse: (text: string) => {
        const value = Number.parseFloat(text.replace("−", "-"))
        return Number.isNaN(value) ? null : value
      },
    })
    const copied = (
      unitKind: string | undefined,
      value: number,
      text: string
    ) => ({ unitKind, value, text })
    // The same kind: the number as it is.
    expect(
      pasteOutcome(control("gain"), copied("gain", 0.4217, "−7.5 dB"))
    ).toEqual({ value: 0.4217 })
    // Another kind, or a kind against none: not at all.
    expect(
      pasteOutcome(control("pan"), copied("gain", 0.4217, "−7.5 dB"))
    ).toEqual({ reason: "Another unit" })
    expect(
      pasteOutcome(control(undefined), copied("gain", 0.4217, "−7.5 dB"))
    ).toEqual({ reason: "Another unit" })
    expect(
      pasteOutcome(control(hzUnit.unitKind), copied(undefined, 3, "3 beats"))
    ).toEqual({ reason: "Another unit" })
    // A bare number goes anywhere, read as if it were typed there.
    expect(
      pasteOutcome(control("decibels"), copied(PLAIN_NUMBER, 12, "12"))
    ).toEqual({ value: 12 })
    expect(
      pasteOutcome(control(PLAIN_NUMBER), copied(PLAIN_NUMBER, 12, "12"))
    ).toEqual({ value: 12 })
    // Two controls that say nothing go by the readout, as before.
    expect(
      pasteOutcome(control(undefined), copied(undefined, 7.5, "7.5"))
    ).toEqual({ value: 7.5 })
    expect(
      pasteOutcome(control(undefined), copied(undefined, 0, "Off"))
    ).toEqual({ reason: "Not a value for this" })
  })

  it("puts the readout on the system clipboard, for other programs", async () => {
    const written = vi.fn(() => Promise.resolve())
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText: written },
    })
    render(
      <ValueContextMenus>
        <Fader aria-label="Volume" value={1} />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Volume"))
    fireEvent.click(item("Copy value"))
    expect(written).toHaveBeenCalledWith("0.0 dB")
    vi.unstubAllGlobals()
  })

  it("opens the control's text entry once the menu has closed, with the focus in it", async () => {
    const user = userEvent.setup()
    render(
      <ValueContextMenus>
        <NumberField aria-label="Tempo" value={128} min={10} max={522} />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Tempo"))
    await user.click(item(/^Type in value/))
    const entry = await screen.findByRole("textbox", { name: "Tempo" })
    await waitFor(() => expect(entry).toHaveFocus())
    expect(entry).toHaveValue("128")
    expect(screen.queryByRole("menu")).toBeNull()
  })

  it("lists first what the control is bound to has to offer", async () => {
    const ran = vi.fn()
    render(
      <ValueContextMenus>
        <ValueContextItems items={[{ title: "Show in mixer", run: ran }]}>
          <LiveKnob name="Send" start={3} />
        </ValueContextItems>
        <LiveKnob name="Other" start={3} />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Send"))
    const entries = within(menu()).getAllByRole("menuitem")
    expect(entries[0]).toHaveTextContent("Show in mixer")
    expect(entries[1]).toHaveTextContent("Reset to default")
    fireEvent.click(entries[0])
    expect(ran).toHaveBeenCalledTimes(1)

    // A control outside the wrapper has only its own entries.
    await rightClick(rootOf("Other"))
    expect(within(menu()).queryByText("Show in mixer")).toBeNull()
  })

  it("greys out what a disabled control cannot do, and still copies", async () => {
    render(
      <ValueContextMenus>
        <LiveKnob name="Cutoff" start={5} disabled />
      </ValueContextMenus>
    )
    await rightClick(rootOf("Cutoff"))
    expect(item(/^Reset to default/)).toHaveAttribute("aria-disabled", "true")
    expect(item(/^Type in value/)).toHaveAttribute("aria-disabled", "true")
    expect(item("Copy value")).not.toHaveAttribute("aria-disabled", "true")
  })

  it("leaves the text entry of a control the menu every text field has", async () => {
    render(
      <ValueContextMenus>
        <LiveKnob name="Cutoff" start={5} />
      </ValueContextMenus>
    )
    fireEvent.keyDown(slider("Cutoff"), { key: "Enter" })
    const entry = screen.getByRole("textbox")
    expect(await rightClick(entry)).toBe(true)
    expect(screen.queryByRole("menu")).toBeNull()
  })

  it("is the app's to give: the kit alone has no menu", async () => {
    render(<LiveKnob name="Cutoff" start={5} />)
    expect(await rightClick(rootOf("Cutoff"))).toBe(true)
    expect(screen.queryByRole("menu")).toBeNull()
  })
})
