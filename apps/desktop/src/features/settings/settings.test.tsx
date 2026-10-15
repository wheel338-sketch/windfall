import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AudioDevice, AudioHost } from "@/bindings"
import { setBackend, type Backend, type StoredAudioSettings } from "@/lib/ipc"
import { useUiStore } from "@/lib/store/ui"
import { refreshEngineStatus } from "@/lib/store/engine"
import { settle, startTestApp } from "@/test/harness"

import {
  bufferOptions,
  bufferSizesFor,
  DEFAULT_NUMBER,
  DEFAULT_TEXT,
  fixedBuffer,
  nameOptions,
  sampleRateOptions,
  sampleRatesFor,
  shownDevice,
  withField,
} from "./audio-options"
import { SettingsDialog } from "./settings-dialog"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

function device(extra: Partial<AudioDevice> = {}): AudioDevice {
  return {
    name: "Speakers",
    isDefault: true,
    sampleRates: [8_000, 44_100, 48_000, 96_000, 384_000],
    minBufferFrames: 64,
    maxBufferFrames: 1024,
    ...extra,
  }
}

/** What WASAPI in shared mode reports: one buffer size, take it or leave it. */
const SHARED = device({ minBufferFrames: 480, maxBufferFrames: 480 })

describe("audio options", () => {
  it("offers the common sample rates the device has, and no others", () => {
    expect(sampleRatesFor(device())).toEqual([44_100, 48_000, 96_000])
    expect(sampleRatesFor(undefined)).toEqual([])
    expect(sampleRateOptions(device(), undefined)).toEqual([
      { value: DEFAULT_NUMBER, label: "Default" },
      { value: 44_100, label: "44.1 kHz" },
      { value: 48_000, label: "48 kHz" },
      { value: 96_000, label: "96 kHz" },
    ])
  })

  it("offers the powers of two inside the device's range, and its ends", () => {
    expect(bufferSizesFor(device())).toEqual([64, 128, 256, 512, 1024])
    expect(
      bufferSizesFor(device({ minBufferFrames: 100, maxBufferFrames: 600 }))
    ).toEqual([100, 128, 256, 512, 600])
    // A driver that does not say gets the usual sizes.
    expect(
      bufferSizesFor(device({ minBufferFrames: null, maxBufferFrames: null }))
    ).toEqual([32, 64, 128, 256, 512, 1024, 2048, 4096])
  })

  it("knows a driver with one fixed buffer, and still lists that size", () => {
    expect(fixedBuffer(SHARED)).toBe(480)
    expect(fixedBuffer(device())).toBeNull()
    expect(bufferSizesFor(SHARED)).toEqual([480])
    expect(bufferOptions(SHARED, undefined, 48_000)).toEqual([
      { value: DEFAULT_NUMBER, label: "Default" },
      { value: 480, label: "480 samples, 10.0 ms" },
    ])
  })

  it("shows a stored value the device does not offer instead of an empty box", () => {
    expect(bufferOptions(SHARED, 80, 48_000).at(-1)).toEqual({
      value: 80,
      label: "80 samples, 1.7 ms, not offered by this device",
    })
    expect(sampleRateOptions(device(), 8_000).at(-1)).toEqual({
      value: 8_000,
      label: "8 kHz, not offered by this device",
    })
    expect(nameOptions([device()], "Unplugged").at(-1)).toEqual({
      value: "Unplugged",
      label: "Unplugged, not found",
    })
  })

  it("adds nothing for the nulls the shell stores for fields left to the system", () => {
    // What `engine_settings` replies before anything was chosen, verbatim.
    const stored = JSON.parse(
      '{"host":null,"device":null,"sampleRate":null,"bufferFrames":null}'
    ) as StoredAudioSettings
    const hosts: AudioHost[] = [
      { name: "WASAPI", isDefault: true, devices: [device()] },
    ]
    expect(nameOptions(hosts, stored.host)).toEqual([
      { value: DEFAULT_TEXT, label: "Default" },
      { value: "WASAPI", label: "WASAPI" },
    ])
    expect(nameOptions(hosts[0].devices, stored.device)).toEqual([
      { value: DEFAULT_TEXT, label: "Default" },
      { value: "Speakers", label: "Speakers" },
    ])
    expect(sampleRateOptions(device(), stored.sampleRate)).toEqual(
      sampleRateOptions(device(), undefined)
    )
    expect(bufferOptions(SHARED, stored.bufferFrames, 48_000)).toEqual(
      bufferOptions(SHARED, undefined, 48_000)
    )
    expect(shownDevice(hosts, stored, null).device?.name).toBe("Speakers")
    // A change sends only what was asked for, never the nulls back.
    expect(withField(stored, "bufferFrames", 256)).toStrictEqual({
      bufferFrames: 256,
    })
    expect(withField(stored, "host", DEFAULT_TEXT)).toStrictEqual({})
  })

  it("changes one field of the request and leaves the rest as asked", () => {
    const request = { host: "WASAPI", sampleRate: 44_100 }
    expect(withField(request, "bufferFrames", 256)).toEqual({
      host: "WASAPI",
      sampleRate: 44_100,
      bufferFrames: 256,
    })
    // "Default" takes the field out again.
    expect(withField(request, "sampleRate", DEFAULT_NUMBER)).toEqual({
      host: "WASAPI",
    })
    expect(withField(request, "host", DEFAULT_TEXT)).toEqual({
      sampleRate: 44_100,
    })
    expect(request).toEqual({ host: "WASAPI", sampleRate: 44_100 })
  })

  it("is about the device asked for, or the one running when none was", () => {
    const usb = device({ name: "USB", isDefault: false })
    const hosts: AudioHost[] = [
      { name: "WASAPI", isDefault: true, devices: [device(), usb] },
      { name: "ASIO", isDefault: false, devices: [usb] },
    ]
    const running = { host: "WASAPI", device: "USB" }
    expect(shownDevice(hosts, {}, running).device?.name).toBe("USB")
    expect(shownDevice(hosts, { device: "Speakers" }, running).device).toBe(
      hosts[0].devices[0]
    )
    expect(shownDevice(hosts, { host: "ASIO" }, running).host?.name).toBe(
      "ASIO"
    )
    expect(shownDevice(hosts, {}, null).device?.name).toBe("Speakers")
  })
})

describe("audio settings in the dialog", () => {
  let backend: Backend
  let stop: () => void

  beforeEach(async () => {
    ;({ backend, stop } = await startTestApp())
  })
  afterEach(() => stop())

  async function open(overrides: Partial<Backend> = {}) {
    backend = { ...backend, ...overrides }
    setBackend(backend)
    await refreshEngineStatus()
    useUiStore.getState().openDialog("settings")
    render(<SettingsDialog />)
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: "Buffer size" })
      ).toBeEnabled()
    )
  }
  const box = (name: string) => screen.getByRole("combobox", { name })

  it("shows what was asked for as Default, apart from what the engine runs at", async () => {
    await open()
    for (const name of [
      "Audio driver",
      "Output device",
      "Sample rate",
      "Buffer size",
    ]) {
      expect(box(name)).toHaveTextContent("Default")
    }
    expect(screen.getByRole("status", { name: "Audio engine status" })).toHaveTextContent(
      "Running on Speakers (simulated) at 48 kHz with 2 output channels and a callback buffer of 256 samples: 5.3 ms per buffer."
    )
  })

  it("sends only the field that was changed, not what the engine reported", async () => {
    const user = userEvent.setup()
    await open()
    const configure = vi.spyOn(backend, "engineConfigure")

    await user.click(box("Sample rate"))
    await user.click(await screen.findByRole("option", { name: "44.1 kHz" }))
    await settle()
    expect(configure).toHaveBeenLastCalledWith({ sampleRate: 44_100 })
    expect(await backend.engineSettings()).toStrictEqual({
      host: null,
      device: null,
      sampleRate: 44_100,
      bufferFrames: null,
      outputChannels: null,
    })
    expect(box("Buffer size")).toHaveTextContent("Default")

    await user.click(box("Buffer size"))
    await user.click(
      await screen.findByRole("option", { name: /^512 samples/ })
    )
    await settle()
    expect(configure).toHaveBeenLastCalledWith({
      sampleRate: 44_100,
      bufferFrames: 512,
    })

    await user.click(box("Sample rate"))
    await user.click(await screen.findByRole("option", { name: "Default" }))
    await settle()
    expect(configure).toHaveBeenLastCalledWith({ bufferFrames: 512 })
  })

  it("changes and restores a persisted live processing-thread count", async () => {
    const user = userEvent.setup()
    await open({ engineSettings: () => Promise.resolve({ processingThreads: 4 }) })
    const configure = vi.spyOn(backend, "engineConfigure")
    expect(box("Audio processing threads")).toHaveTextContent("4")
    await user.click(box("Audio processing threads"))
    await user.click(await screen.findByRole("option", { name: "1 (serial)" }))
    await settle()
    expect(configure).toHaveBeenLastCalledWith({ processingThreads: 1 })
    await user.click(box("Buffer size"))
    await user.click(await screen.findByRole("option", { name: /^512 samples/ }))
    await settle()
    expect(configure).toHaveBeenLastCalledWith({
      processingThreads: 1,
      bufferFrames: 512,
    })
    await user.click(box("Audio processing threads"))
    await user.click(await screen.findByRole("option", { name: "Automatic" }))
    await settle()
    expect(configure).toHaveBeenLastCalledWith({ bufferFrames: 512 })
  })

  it("keeps the saved count and shows worker-preparation refusals", async () => {
    const user = userEvent.setup()
    await open({
      engineSettings: () => Promise.resolve({ processingThreads: 4 }),
      engineConfigure: async () => ({
        ...(await backend.engineStatus()),
        error: "Could not create audio processing workers",
      }),
    })
    await user.click(box("Audio processing threads"))
    await user.click(await screen.findByRole("option", { name: "8" }))
    await waitFor(() => {
      expect(box("Audio processing threads")).toHaveTextContent("4")
      expect(screen.getByRole("status", { name: "Audio engine status" })).toHaveTextContent(
        "Could not create audio processing workers"
      )
    })
  })

  it("lists only real drivers and devices when the shell stored nulls", async () => {
    const user = userEvent.setup()
    await open({
      engineSettings: () =>
        Promise.resolve(
          JSON.parse(
            '{"host":null,"device":null,"sampleRate":null,"bufferFrames":null}'
          ) as StoredAudioSettings
        ),
    })
    const listed = async (name: string) => {
      await user.click(box(name))
      const options = within(await screen.findByRole("listbox")).getAllByRole("option")
      const texts = options.map((item) => item.textContent)
      await user.keyboard("{Escape}")
      return texts
    }
    expect(await listed("Audio driver")).toEqual(["Default", "WASAPI", "ASIO"])
    expect(await listed("Output device")).toEqual([
      "Default",
      "Speakers (simulated)",
      "Headphones (simulated)",
    ])
    expect(await listed("Sample rate")).toEqual([
      "Default",
      "44.1 kHz",
      "48 kHz",
      "96 kHz",
    ])
    expect((await listed("Buffer size")).join("|")).not.toMatch(
      /null|not offered|not found/
    )
  })

  it("offers only the common rates and the sizes the device takes", async () => {
    const user = userEvent.setup()
    await open()
    await user.click(box("Sample rate"))
    expect(
      within(await screen.findByRole("listbox")).getAllByRole("option").map((item) => item.textContent)
    ).toEqual(["Default", "44.1 kHz", "48 kHz", "96 kHz"])
    await user.keyboard("{Escape}")

    await user.click(box("Buffer size"))
    expect(
      within(await screen.findByRole("listbox")).getAllByRole("option").map((item) => item.textContent)
    ).toEqual([
      "Default",
      "64 samples, 1.3 ms",
      "128 samples, 2.7 ms",
      "256 samples, 5.3 ms",
      "512 samples, 10.7 ms",
      "1024 samples, 21.3 ms",
      "2048 samples, 42.7 ms",
      "4096 samples, 85.3 ms",
    ])
  })

  it("says plainly that a driver has one fixed buffer, and why", async () => {
    const hosts: AudioHost[] = [
      { name: "WASAPI", isDefault: true, devices: [SHARED] },
    ]
    await open({
      engineDevices: () => Promise.resolve(hosts),
      engineStatus: () =>
        Promise.resolve({
          running: true,
          host: "WASAPI",
          device: "Speakers",
          sampleRate: 48_000,
          bufferFrames: 480,
          latencyMs: 10,
          latencyFrames: 0,
          error: null,
        }),
    })
    expect(
      screen.getByText(
        "This driver advertises a buffer of 480 samples. The running buffer below can differ with sample rate. Smaller buffers need an ASIO driver, which this build does not include."
      )
    ).toBeVisible()
    expect(box("Buffer size")).toHaveTextContent("Default")
  })

  it.each([
    { sampleRate: 44_100, bufferFrames: 441, rateLabel: "44.1 kHz" },
    { sampleRate: 48_000, bufferFrames: 480, rateLabel: "48 kHz" },
    { sampleRate: 96_000, bufferFrames: 960, rateLabel: "96 kHz" },
  ])(
    "reports the driver's actual callback at $sampleRate Hz separately from its advertised buffer",
    async ({ sampleRate, bufferFrames, rateLabel }) => {
      await open({
        engineDevices: () =>
          Promise.resolve([{ name: "WASAPI", isDefault: true, devices: [SHARED] }]),
        engineSettings: () => Promise.resolve({ sampleRate, bufferFrames: 128 }),
        engineStatus: () =>
          Promise.resolve({
            running: true,
            host: "WASAPI",
            device: "Speakers",
            sampleRate,
            bufferFrames,
            latencyMs: 10,
            latencyFrames: 0,
            error: null,
          }),
      })
      expect(
        screen.getByText(`Running on Speakers at ${rateLabel} with 2 output channels and a callback buffer of ${bufferFrames} samples: 10.0 ms per buffer.`)
      ).toBeVisible()
      expect(screen.getByText(/This driver advertises a buffer of 480 samples/)).toBeVisible()
      expect(box("Buffer size")).toHaveTextContent("128 samples")
      expect(screen.queryByText(/ms of output latency/)).not.toBeInTheDocument()
    }
  )

  it("opens at the top of the form and jumps to a section from the list", async () => {
    const user = userEvent.setup()
    await open()
    const body = document.querySelector<HTMLElement>("[data-slot=settings-body]")!
    // Focus starts at the top of the form rather than on a control further down.
    await waitFor(() => expect(body).toHaveFocus())
    expect(body.scrollTop).toBe(0)
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument()

    const sections = screen.getByRole("navigation", { name: "Settings sections" })
    const keyboard = body.querySelector<HTMLElement>("[data-settings-section=keyboard]")!
    Object.defineProperty(keyboard, "offsetTop", { configurable: true, value: 640 })
    await user.click(within(sections).getByRole("button", { name: "Keyboard" }))
    expect(body.scrollTop).toBe(640)
    expect(keyboard).toHaveFocus()
  })

  it("shows a stored request the device cannot do, so it can be put right", async () => {
    await open({
      engineSettings: () => Promise.resolve({ bufferFrames: 80 }),
    })
    expect(box("Buffer size")).toHaveTextContent(
      "80 samples, 1.7 ms, not offered by this device"
    )
  })
})
