import { fireEvent, render, screen, within } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import {
  EFFECT_KINDS,
  effectDescriptor,
  instrumentDescriptor,
  type ParamDescriptor,
} from "./descriptors"
import { GenericParamEditor } from "./generic-param-editor"
import { humanizePath, paramGroups, shortLabels } from "./groups"

const synth = instrumentDescriptor("subtractiveSynth")

const outline = (descriptor: ParamDescriptor) =>
  paramGroups(descriptor).map((group) => [
    group.title,
    group.params.map((param) => param.label),
  ])

describe("paramGroups", () => {
  it("turns a path into a heading", () => {
    expect(humanizePath(["filter"])).toBe("Filter")
    expect(humanizePath(["ampEnvelope"])).toBe("Amp envelope")
    expect(humanizePath(["oscillators", "0"])).toBe("Oscillator 1")
    expect(humanizePath(["lfos", "1"])).toBe("LFO 2")
    expect(humanizePath(["lowShelf"])).toBe("Low shelf")
    expect(humanizePath(["peak3"])).toBe("Peak 3")
  })

  it("groups the synth the way its settings are nested", () => {
    expect(outline(synth)).toEqual([
      [
        "Oscillator 1",
        ["Waveform", "Level", "Coarse", "Fine", "Pulse width", "Pan"],
      ],
      [
        "Oscillator 2",
        ["Waveform", "Level", "Coarse", "Fine", "Pulse width", "Pan"],
      ],
      [
        "Oscillator 3",
        ["Waveform", "Level", "Coarse", "Fine", "Pulse width", "Pan"],
      ],
      ["Unison", ["Voices", "Detune", "Spread"]],
      [
        "Filter",
        [
          "Mode",
          "Slope",
          "Cutoff",
          "Resonance",
          "Key tracking",
          "Envelope amount",
          "Velocity",
          "Drive",
        ],
      ],
      ["Amp envelope", ["Attack", "Decay", "Sustain", "Release"]],
      ["Filter envelope", ["Attack", "Decay", "Sustain", "Release"]],
      [
        "LFO 1",
        [
          "Shape",
          "Rate",
          "To pitch",
          "To cutoff",
          "To volume",
          "To pulse width",
        ],
      ],
      [
        "LFO 2",
        [
          "Shape",
          "Rate",
          "To pitch",
          "To cutoff",
          "To volume",
          "To pulse width",
        ],
      ],
      [
        "General",
        [
          "Voice mode",
          "Glide",
          "Polyphony",
          "Volume velocity",
          "Volume",
          "Pan",
        ],
      ],
    ])
  })

  it("names an equalizer's bands and their switches", () => {
    const groups = outline(effectDescriptor("eq"))
    expect(groups.map(([title]) => title)).toEqual([
      "Low cut",
      "Low shelf",
      "Peak 1",
      "Peak 2",
      "Peak 3",
      "High shelf",
      "High cut",
      "General",
    ])
    expect(groups[0][1]).toEqual(["Enabled", "Frequency", "Q", "Slope"])
    expect(groups[2][1]).toEqual(["Enabled", "Frequency", "Gain", "Q"])
    expect(groups[7][1]).toEqual(["Output gain"])
  })

  it("gives a flat list of settings no heading", () => {
    const groups = paramGroups(effectDescriptor("compressor"))
    expect(groups).toHaveLength(1)
    expect(groups[0].title).toBeNull()
    expect(groups[0].params.map((param) => param.label)).toEqual([
      "Threshold",
      "Ratio",
      "Attack",
      "Release",
      "Knee",
      "Makeup",
      "Auto makeup",
      "Detector",
      "Mix",
      "External sidechain",
    ])
  })

  it("places every setting of every descriptor exactly once", () => {
    for (const descriptor of [
      synth,
      ...EFFECT_KINDS.map((kind) => effectDescriptor(kind)),
    ]) {
      const placed = paramGroups(descriptor).flatMap((group) =>
        group.params.map((param) => param.info.id)
      )
      expect([...placed].sort()).toEqual(
        descriptor.params.map((info) => info.id).sort()
      )
      for (const group of paramGroups(descriptor)) {
        for (const param of group.params) expect(param.label).not.toBe("")
      }
    }
  })

  it("keeps a name that shares nothing with its neighbours", () => {
    const infos = effectDescriptor("limiter").params
    expect(shortLabels(infos, null)).toEqual([
      "Ceiling",
      "Input gain",
      "Release",
      "Look-ahead",
      "Mode",
      "Threshold",
      "Ratio",
      "Knee",
      "Attack",
      "Gate",
      "Gate release",
    ])
  })
})

describe("GenericParamEditor", () => {
  it.each(["subtractiveSynth", ...EFFECT_KINDS] as const)(
    "%s: gives every setting a control",
    (kind) => {
      const descriptor: ParamDescriptor =
        kind === "subtractiveSynth" ? synth : effectDescriptor(kind)
      render(
        <GenericParamEditor
          descriptor={descriptor}
          params={descriptor.defaults}
          setParam={() => undefined}
        />
      )
      const editor = screen.getByRole("group", {
        name: `${descriptor.name} settings`,
      })
      const shown = [
        ...editor.querySelectorAll<HTMLElement>("[data-param]"),
      ].map((element) => element.dataset.param)
      expect(shown).toHaveLength(descriptor.params.length)
      expect(new Set(shown)).toEqual(
        new Set(descriptor.params.map((info) => info.id))
      )
    }
  )

  it("titles its groups and shortens the labels inside them", () => {
    render(
      <GenericParamEditor
        descriptor={synth}
        params={synth.defaults}
        setParam={() => undefined}
      />
    )
    const filter = screen.getByRole("region", { name: "Filter" })
    expect(
      within(filter).getByRole("heading", { name: "Filter" })
    ).toBeVisible()
    expect(within(filter).getByRole("slider", { name: "Cutoff" })).toBeVisible()
    expect(
      within(filter).getByRole("radiogroup", { name: "Filter mode" })
    ).toBeVisible()
    expect(within(filter).getByText("Mode")).toBeVisible()

    const second = screen.getByRole("region", { name: "Oscillator 2" })
    expect(
      within(second).getByRole("slider", { name: "Osc 2 level" })
    ).toHaveAttribute("aria-valuetext", "0%")
  })

  it("sends changes by index, one gesture per move", () => {
    const setParam = vi.fn()
    const compressor = effectDescriptor("compressor")
    render(
      <GenericParamEditor
        descriptor={compressor}
        params={compressor.defaults}
        setParam={setParam}
      />
    )
    const ratio = screen.getByRole("slider", { name: "Ratio" })
    fireEvent.keyDown(ratio, { key: "End" })
    fireEvent.keyUp(ratio, { key: "End" })
    expect(setParam).toHaveBeenLastCalledWith(1, 100, expect.any(Number))

    fireEvent.click(screen.getByRole("button", { name: "Auto makeup" }))
    expect(setParam).toHaveBeenLastCalledWith(6, 1, expect.any(Number))
    fireEvent.click(screen.getByRole("radio", { name: "RMS" }))
    expect(setParam).toHaveBeenLastCalledWith(7, 1, expect.any(Number))
  })

  it("is disabled without settings", () => {
    const reverb = effectDescriptor("reverb")
    render(
      <GenericParamEditor
        descriptor={reverb}
        params={null}
        setParam={() => undefined}
      />
    )
    for (const knob of screen.getAllByRole("slider")) {
      expect(knob).toHaveAttribute("aria-disabled", "true")
    }
  })
})
