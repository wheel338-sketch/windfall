import { afterEach, describe, expect, it } from "vitest"
import { beginExportRequest, receiveExportRequest, releaseExportIntent, setExportIntent, useExportRequestStore } from "./requests"
import { useProjectStore } from "@/lib/store/project"

afterEach(() => useExportRequestStore.setState({ intent: null, request: null }))
describe("export request correlation", () => {
  it("refuses foreign, missing, wrong-path and old IDs even when the destination is reused", () => {
    const before = useProjectStore.getState()
    setExportIntent("guide")
    const old = beginExportRequest("same.wav")
    setExportIntent("guide")
    const current = beginExportRequest("same.wav")
    const done = { path: "same.wav", done: true, fraction: 1, error: null, droppedClips: 0 }
    for (const event of [{ ...done, request: old }, done, { ...done, request: "another-window" }, { ...done, path: "other.wav", request: current }]) {
      receiveExportRequest(event)
      expect(useExportRequestStore.getState().request?.status).toBe("pending")
    }
    receiveExportRequest({ ...done, request: current })
    expect(useExportRequestStore.getState().request).toMatchObject({ owner: "guide", status: "succeeded", id: current })
    expect(useProjectStore.getState()).toBe(before)
  })
  it("cancellation and errors require a fresh request and an old success cannot revive them", () => {
    setExportIntent("guide")
    const id = beginExportRequest("same.wav")
    const done = { path: "same.wav", request: id, done: true, fraction: 1, error: null, droppedClips: 0 }
    receiveExportRequest({ ...done, cancelled: true })
    receiveExportRequest(done)
    expect(useExportRequestStore.getState().request?.status).toBe("cancelled")
    setExportIntent("guide")
    const retry = beginExportRequest("same.wav")
    receiveExportRequest({ ...done, request: retry, error: "Disk full" })
    receiveExportRequest({ ...done, request: retry })
    expect(useExportRequestStore.getState().request).toMatchObject({ status: "failed", error: "Disk full" })
    setExportIntent("guide")
    releaseExportIntent("another-guide")
    expect(useExportRequestStore.getState().intent).toBe("guide")
    releaseExportIntent("guide")
    expect(useExportRequestStore.getState().intent).toBeNull()
  })
})
