import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import { runAction } from "@/lib/actions"
import { SimDocument } from "@/lib/ipc/sim/document"
import { redo, undo, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import { TimingSection } from "./inspector/timing-section"
import { channel, history, project, startRack } from "./test-utils"

let app: Awaited<ReturnType<typeof startRack>>
beforeEach(async () => {
  app = await startRack()
})
afterEach(() => app.stop())

function MountedTiming() {
  const current = useProjectStore((state) => state.project.channels[0])
  return <TimingSection channel={current} />
}
const flush = () => act(settle)

/** Picks an entry of a setting's presets and steps menu. */
async function pick(setting: string, entry: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `${setting} presets and steps` })
  )
  fireEvent.click(await screen.findByRole("menuitem", { name: entry }))
  await flush()
}

it("composes mounted swing, gate and signed-shift controls without overwriting each other or notes, with reset, history and file persistence", async () => {
  useUiStore.getState().selectChannel(channel("Kick").id)
  render(<MountedTiming />)
  const before = structuredClone(project())
  const cursor = history().cursor
  await pick("Swing mix", "Half")
  await pick("Gate", "Quarter")
  await pick("Shift", "16th early")
  expect(channel("Kick").timing).toEqual({
    swingMix: 0.5,
    gateTicks: 960,
    shiftTicks: -240,
  })
  expect(history().cursor).toBe(cursor + 3)
  expect(project().patterns).toEqual(before.patterns)
  expect(project().channels.slice(1)).toEqual(before.channels.slice(1))
  const doc = SimDocument.create((await app.backend.documentSnapshot()).project)
  const reopened = SimDocument.open(doc.fileText())
  expect(reopened.project().channels[0].timing).toEqual(channel("Kick").timing)
  doc.dispose()
  reopened.dispose()
  await act(async () => {
    await undo()
  })
  expect(channel("Kick").timing).toEqual({
    swingMix: 0.5,
    gateTicks: 960,
    shiftTicks: 0,
  })
  await act(async () => {
    await redo()
  })
  await pick("Shift", "8th late")
  expect(channel("Kick").timing).toEqual({
    swingMix: 0.5,
    gateTicks: 960,
    shiftTicks: 480,
  })
  const configured = structuredClone(project())
  const configuredCursor = history().cursor
  await act(async () => {
    await runAction("channel.resetTiming")
  })
  expect(
    channel("Kick").timing ?? { swingMix: 1, gateTicks: 0, shiftTicks: 0 }
  ).toEqual({ swingMix: 1, gateTicks: 0, shiftTicks: 0 })
  expect(history().cursor).toBe(configuredCursor + 1)
  expect(project().patterns).toEqual(before.patterns)
  await act(async () => {
    await undo()
  })
  expect(project()).toEqual(configured)
})
