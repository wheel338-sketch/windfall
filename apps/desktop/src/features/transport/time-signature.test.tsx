import userEvent from "@testing-library/user-event"
import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"
import { startTestApp } from "@/test/harness"
import { undo, useProjectStore } from "@/lib/store/project"
import { TimeSignatureField } from "./time-signature"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  app = await startTestApp()
})
afterEach(() => {
  cleanup()
  app.stop()
})
const meter = () => useProjectStore.getState().project.settings.timeSignature
const cursor = () => useProjectStore.getState().history.cursor

it("the transport bar's time signature edits the project meter in undoable steps that are saved", async () => {
  expect(meter()).toEqual({ numerator: 4, denominator: 4 })
  const before = cursor()
  render(<TimeSignatureField />)
  await userEvent.click(
    screen.getByRole("button", { name: "Time signature: 4 over 4" })
  )
  // The scale buttons first: the popover stays open for them.
  await userEvent.click(
    await screen.findByRole("button", { name: "Double beat unit" })
  )
  await waitFor(() => expect(meter()).toEqual({ numerator: 4, denominator: 8 }))
  await userEvent.click(
    screen.getByRole("button", { name: "Double beats per bar" })
  )
  await waitFor(() => expect(meter()).toEqual({ numerator: 8, denominator: 8 }))
  // Then a number picked from the list.
  await userEvent.click(screen.getByRole("combobox", { name: "Beats per bar" }))
  await userEvent.click(await screen.findByRole("option", { name: "7" }))
  await waitFor(() => expect(meter()).toEqual({ numerator: 7, denominator: 8 }))
  // One history step each, and the field shows what the project has.
  expect(cursor()).toBe(before + 3)
  expect(
    screen.getByRole("button", { name: "Time signature: 7 over 8" })
  ).toHaveTextContent("7/8")
  await act(async () => {
    await undo()
  })
  expect(meter()).toEqual({ numerator: 8, denominator: 8 })
  // Saved with the project and read back from the file.
  await app.backend.projectSave("/meter.windfall")
  await app.backend.projectNew()
  await waitFor(() => expect(meter()).toEqual({ numerator: 4, denominator: 4 }))
  await app.backend.projectOpen("/meter.windfall")
  await waitFor(() => expect(meter()).toEqual({ numerator: 8, denominator: 8 }))
})
