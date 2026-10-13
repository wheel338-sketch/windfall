import { StrictMode } from "react"
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import {
  createDefaultChannelVoiceSettings,
  sanitizeChannelVoiceSettings,
} from "./settings"
import { ChannelVoicePanel } from "./voice-panel"

afterEach(cleanup)

it("reports one complete draft per edit in Strict Mode, preserving earlier edits", () => {
  const onChange = vi.fn()
  render(
    <StrictMode>
      <ChannelVoicePanel onChange={onChange} />
    </StrictMode>
  )
  expect(onChange).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("switch", { name: "Enable note echo" }))
  expect(onChange).toHaveBeenCalledTimes(1)
  expect(onChange.mock.calls[0][0].echo.enabled).toBe(true)
  fireEvent.click(screen.getByRole("switch", { name: "Mono legato" }))
  expect(onChange).toHaveBeenCalledTimes(2)
  expect(onChange.mock.calls[1][0].echo.enabled).toBe(true)
  expect(onChange.mock.calls[1][0].polyphony.monoLegato).toBe(true)
})

it("edits tempo echo timing and a pitch envelope without mutating initial settings", () => {
  const initial = createDefaultChannelVoiceSettings()
  const onChange = vi.fn()
  render(<ChannelVoicePanel initialSettings={initial} onChange={onChange} />)
  fireEvent.click(screen.getByRole("button", { name: "Tempo" }))
  expect(onChange.mock.lastCall?.[0].echo.time).toEqual({
    unit: "division",
    division: "eighth",
  })
  fireEvent.click(screen.getByRole("switch", { name: "Enable pitch envelope" }))
  expect(onChange.mock.lastCall?.[0].envelopes.pitch.enabled).toBe(true)
  fireEvent.keyDown(
    screen.getByRole("slider", { name: "Pitch envelope depth" }),
    { key: "ArrowUp" }
  )
  expect(onChange.mock.lastCall?.[0].envelopes.pitch.depth).toBeGreaterThan(0)
  expect(initial).toEqual(createDefaultChannelVoiceSettings())
})

it("creates independent defaults and sanitizes finite limits and integer fields", () => {
  const value = createDefaultChannelVoiceSettings()
  value.envelopes.filter.depth = 20
  expect(value.envelopes.pitch.depth).toBe(0)
  value.echo.feedback = 2
  value.echo.repeats = 99
  value.echo.pitchSemitones = -200
  value.polyphony.maxVoices = 0
  value.polyphony.portamentoMs = Number.NaN
  value.arpeggiator.rangeOctaves = 2.7
  const clean = sanitizeChannelVoiceSettings(value)
  expect(clean.echo).toMatchObject({
    feedback: 0.95,
    repeats: 8,
    pitchSemitones: -48,
  })
  expect(clean.polyphony).toMatchObject({ maxVoices: 1, portamentoMs: 0 })
  expect(clean.arpeggiator.rangeOctaves).toBe(2)
  expect(clean.envelopes.filter.depth).toBe(8)
})

it("edits the channel filter, the resonance envelope and a target LFO as complete drafts", () => {
  const onChange = vi.fn()
  render(<ChannelVoicePanel onChange={onChange} />)
  fireEvent.click(screen.getByRole("switch", { name: "Enable channel filter" }))
  expect(onChange.mock.lastCall?.[0].envelopes.filterSettings).toEqual({
    enabled: true,
    mode: "lowPass",
    cutoffHz: 2000,
    resonance: 0,
  })
  fireEvent.click(screen.getByRole("button", { name: "Band-pass" }))
  expect(onChange.mock.lastCall?.[0].envelopes.filterSettings.mode).toBe(
    "bandPass"
  )
  fireEvent.keyDown(
    screen.getByRole("slider", { name: "Channel filter resonance" }),
    { key: "ArrowUp" }
  )
  expect(
    onChange.mock.lastCall?.[0].envelopes.filterSettings.resonance
  ).toBeGreaterThan(0)

  fireEvent.click(
    screen.getByRole("switch", { name: "Enable resonance envelope" })
  )
  expect(onChange.mock.lastCall?.[0].envelopes.resonance.enabled).toBe(true)
  // Earlier edits stay in every later draft.
  expect(onChange.mock.lastCall?.[0].envelopes.filterSettings.mode).toBe(
    "bandPass"
  )

  fireEvent.click(screen.getByRole("switch", { name: "Enable volume LFO" }))
  fireEvent.keyDown(screen.getByRole("slider", { name: "Volume LFO depth" }), {
    key: "ArrowUp",
  })
  const lfos = onChange.mock.lastCall?.[0].envelopes.lfos
  expect(lfos.volume.enabled).toBe(true)
  expect(lfos.volume.depth).toBeGreaterThan(0)
  // The other targets keep their defaults and their own switches.
  expect(lfos.pan).toEqual({
    enabled: false,
    shape: "sine",
    rateHz: 5,
    depth: 0,
    delayMs: 0,
    retrigger: false,
  })
  for (const target of ["pan", "pitch", "cutoff", "resonance"])
    expect(
      screen.getByRole("switch", { name: `Enable ${target} LFO` })
    ).not.toBeChecked()
})

it("leaves untouched optional settings out of an edit and keeps a layer through one", () => {
  const initial = createDefaultChannelVoiceSettings()
  const onChange = vi.fn()
  render(
    <ChannelVoicePanel
      initialSettings={{
        ...initial,
        layer: { enabled: true, children: [7, 3] },
      }}
      onChange={onChange}
    />
  )
  fireEvent.click(screen.getByRole("switch", { name: "Enable note echo" }))
  const sent = onChange.mock.lastCall?.[0]
  expect(sent.layer).toEqual({ enabled: true, children: [7, 3] })
  expect(sent.envelopes).not.toHaveProperty("filterSettings")
  expect(sent.envelopes).not.toHaveProperty("lfos")
  expect(sent.envelopes).not.toHaveProperty("resonance")
})

it("hides the sampler-only targets on an instrument channel", () => {
  render(<ChannelVoicePanel kind="instrument" onChange={vi.fn()} />)
  expect(
    screen.queryByRole("switch", { name: "Enable channel filter" })
  ).toBeNull()
  expect(
    screen.queryByRole("switch", { name: "Enable resonance envelope" })
  ).toBeNull()
  expect(screen.queryByRole("switch", { name: "Enable volume LFO" })).toBeNull()
  expect(screen.getByRole("switch", { name: "Enable pitch LFO" })).toBeVisible()
  expect(screen.queryByRole("button", { name: "Volume" })).toBeNull()
})

it("follows the saved settings again once its own save has settled", async () => {
  const saved = createDefaultChannelVoiceSettings()
  let finish: () => void = () => {}
  const onChange = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve
      })
  )
  const { rerender } = render(
    <ChannelVoicePanel initialSettings={saved} onChange={onChange} />
  )
  const echo = () => screen.getByRole("switch", { name: "Enable note echo" })
  fireEvent.click(echo())
  expect(echo()).toBeChecked()
  // The save was refused: the saved settings did not change, and the panel
  // shows them again instead of its own draft.
  finish()
  await waitFor(() => expect(echo()).not.toBeChecked())

  // An undo or a load replaces what the panel shows without a remount.
  rerender(
    <ChannelVoicePanel
      initialSettings={{ ...saved, echo: { ...saved.echo, enabled: true } }}
      onChange={onChange}
    />
  )
  expect(echo()).toBeChecked()
})

it("sanitizes the filter, the target LFOs and the layer children", () => {
  const value = createDefaultChannelVoiceSettings()
  const idle = {
    enabled: false,
    shape: "sine",
    rateHz: 5,
    depth: 0,
    delayMs: 0,
    retrigger: false,
  } as const
  const clean = sanitizeChannelVoiceSettings({
    ...value,
    envelopes: {
      ...value.envelopes,
      filterSettings: {
        enabled: true,
        mode: "notch",
        cutoffHz: 1e9,
        resonance: -1,
      },
      resonance: { ...value.envelopes.pan, depth: 9 },
      lfos: {
        volume: {
          enabled: true,
          shape: "square",
          rateHz: 500,
          depth: -7,
          delayMs: 1e9,
          retrigger: true,
        },
        pan: { ...idle, rateHz: Number.NaN, delayMs: Number.NaN },
        pitch: idle,
        cutoff: idle,
        resonance: idle,
      },
    },
    layer: {
      enabled: true,
      children: [
        4,
        4,
        2.5,
        -1,
        ...Array.from({ length: 20 }, (_, index) => 10 + index),
      ],
    },
  })
  expect(clean.envelopes.filterSettings).toMatchObject({
    cutoffHz: 20_000,
    resonance: 0,
  })
  expect(clean.envelopes.resonance?.depth).toBe(1)
  expect(clean.envelopes.lfos?.volume).toMatchObject({ rateHz: 30, depth: -1 })
  expect(clean.envelopes.lfos?.pan.rateHz).toBe(5)
  expect(clean.envelopes.lfos?.volume).toMatchObject({
    delayMs: 20_000,
    retrigger: true,
  })
  expect(clean.envelopes.lfos?.pan).toMatchObject({
    delayMs: 0,
    retrigger: false,
  })
  expect(clean.layer?.children).toHaveLength(16)
  expect(clean.layer?.children.slice(0, 2)).toEqual([4, 10])
})
