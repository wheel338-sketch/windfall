import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle } from "@/test/harness"
import { channel, startRoll, undoSteps } from "@/features/piano-roll/test-utils"
import { DEFAULT_EXPRESSION_MAP, ExpressionSection } from "./expression-section"

let roll: Awaited<ReturnType<typeof startRoll>>
function Mounted() {
  const selected = useProjectStore((state) => state.project.channels.find((item) => item.name === "Kick")!)
  return <ExpressionSection key={selected.id} channel={selected} />
}
beforeEach(async () => { roll = await startRoll(); render(<Mounted />) })
afterEach(() => { cleanup(); roll.stop(); vi.restoreAllMocks() })

it("edits a real channel map once, saves/reopens and rejects an obsolete review", async () => {
  const user = userEvent.setup()
  const before = undoSteps()
  const original = channel("Kick").expressionMap ?? DEFAULT_EXPRESSION_MAP
  await user.click(screen.getByLabelText("Modulation X · plugin"))
  await user.click(await screen.findByRole("option", { name: "Pressure" }))
  fireEvent.change(screen.getByLabelText("Modulation X depth (%)"), { target: { value: "25" } })
  await user.click(screen.getByRole("switch", { name: "Invert Modulation X" }))
  fireEvent.change(screen.getByLabelText("Plugin bend range (semitones)"), { target: { value: "12" } })
  await act(async () => { await user.click(screen.getByRole("button", { name: "Apply map" })); await settle() })
  const changed = structuredClone(channel("Kick").expressionMap!)
  expect(changed).toMatchObject({ modulationX: { plugin: "pressure", amount: .25, invert: true }, bendRangeSemitones: 12 })
  expect(undoSteps()).toBe(before + 1)
  await roll.backend.projectSave("/projects/expression-map.windfall")
  await act(async () => { await undo(); await settle() })
  expect(channel("Kick").expressionMap ?? DEFAULT_EXPRESSION_MAP).toEqual(original)
  await act(async () => { await redo(); await settle() })
  expect(channel("Kick").expressionMap).toEqual(changed)
  await act(async () => { await undo(); await roll.backend.projectOpen("/projects/expression-map.windfall"); await settle() })
  expect(channel("Kick").expressionMap).toEqual(changed)
  await user.click(screen.getByRole("button", { name: "Reload map" }))
  await act(async () => { await dispatch({ type: "updateChannel", id: channel("Kick").id, patch: { name: "Kick" } }); await settle() })
  // Native captured-map refusal is also exercised with a genuinely raced map.
  const expected = structuredClone(channel("Kick").expressionMap!)
  await act(async () => { await dispatch({ type: "setChannelExpressionMap", id: channel("Kick").id, map: DEFAULT_EXPRESSION_MAP }); await settle() })
  const after = useProjectStore.getState()
  expect(await dispatch({ type: "setChannelExpressionMap", id: channel("Kick").id, expected, map: changed })).toBeNull()
  expect(useProjectStore.getState()).toBe(after)
  expect(screen.getByRole("button", { name: "Apply map" })).toBeDisabled()
})
