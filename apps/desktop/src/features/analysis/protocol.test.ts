import { describe, expect, it, vi } from "vitest"
import { invoke } from "@tauri-apps/api/core"
import { createTauriBackend } from "@/lib/ipc/tauri"
import { decimal, validRange } from "./types"
import { unavailableAnalysis } from "./unavailable"
import { retireJob } from "./retire"
import nativeCommands from "../../../src-tauri/src/commands.rs?raw"
import frontendCommands from "@/lib/ipc/tauri.ts?raw"
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: vi.fn() }))
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }))

describe("analysis native protocol", () => {
  it("registers every invoked analysis command in the desktop IPC handler", () => {
    const invoked = new Set(
      Array.from(
        frontendCommands.matchAll(/call\("(analysis_[a-z_]+)"/g),
        (match) => match[1]
      )
    )
    expect(invoked.size).toBeGreaterThan(0)
    const handler = nativeCommands.match(
      /tauri::generate_handler!\[([\s\S]*?)\]/
    )
    expect(handler, "Desktop IPC handler must exist").not.toBeNull()
    const registered = new Set(handler![1].match(/[a-z_]+/g))
    expect([...invoked].filter((command) => !registered.has(command))).toEqual(
      []
    )
  })
  it("reports the exact retained job identity on immediate cleanup refusal", async () => {
    const api = unavailableAnalysis()
    vi.spyOn(api, "analysisStatus").mockResolvedValue({
      job: "9007199254740993",
      ticket: "2",
      request: "3",
      sequence: "4",
      status: "cancelled",
      completedWork: "0",
      maximumWork: "1",
      failure: null,
    })
    vi.spyOn(api, "analysisForget").mockRejectedValue(
      new Error("owned temporary deletion refused")
    )
    await expect(retireJob(api, "9007199254740993")).rejects.toThrow(
      "Analysis job 9007199254740993 remains retained: Owned temporary deletion refused"
    )
  })
  it("preserves exact decimal identifiers and forwards refusals without successful fallback", async () => {
    const native = createTauriBackend()
    const caller = { generation: 1, token: "analysis-fixture-window" }
    vi.mocked(invoke).mockResolvedValue(null).mockResolvedValueOnce({ generation: 1, caller })
    const request = {
      ticket: "9007199254740993",
      request: "18446744073709551614",
      replaceOriginal: false,
    }
    await native.analysisApply(request)
    expect(invoke).toHaveBeenNthCalledWith(1, "window_document_snapshot", undefined)
    expect(invoke).toHaveBeenLastCalledWith("analysis_apply", { request, caller })
    await native.analysisReview(request.ticket)
    expect(invoke).toHaveBeenLastCalledWith("analysis_review", {
      ticket: request.ticket,
      caller,
    })
    await native.analysisRetryCleanup(request.ticket)
    expect(invoke).toHaveBeenLastCalledWith("analysis_retry_cleanup", {
      job: request.ticket,
      caller,
    })
    vi.mocked(invoke).mockRejectedValue(
      "analysis:unavailable: No inference algorithm is installed."
    )
    await expect(native.analysisStatus(request.ticket)).rejects.toThrow(
      "No inference algorithm"
    )
  })
  it("refuses browser inference, import, review and apply honestly", async () => {
    const browser = unavailableAnalysis()
    expect(await browser.analysisCapability()).toMatchObject({
      native: false,
      available: false,
      models: [],
    })
    await expect(browser.analysisStatus("1")).rejects.toThrow("desktop app")
    await expect(browser.analysisReview("1")).rejects.toThrow("desktop app")
    await expect(
      browser.analysisApply({
        ticket: "1",
        request: "2",
        replaceOriginal: true,
      })
    ).rejects.toThrow("desktop app")
  })
  it("checks ranges beyond JS safe integers and refuses aliases/overflow", () => {
    expect(validRange("9007199254740993", "9007199254740994")).toBe(true)
    expect(validRange("9007199254740994", "9007199254740993")).toBe(false)
    for (const value of [
      "01",
      "-1",
      "+1",
      "1.0",
      "18446744073709551616",
      "1e3",
      "",
    ])
      expect(decimal(value)).toBe(false)
    expect(decimal("18446744073709551615")).toBe(true)
  })
})
