import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import { Section } from "./parts"
import {
  directionSteps,
  factorSteps,
  stepItems,
  StepsMenus,
  type SettingSteps,
} from "./value-steps"

afterEach(cleanup)

function gainSteps(run = vi.fn()): SettingSteps {
  return {
    name: "Gain",
    groups: [
      [
        { title: "Quiet", run: () => run("quiet") },
        { title: "Unity", run: () => run("unity"), disabled: true },
      ],
      directionSteps((direction) => run(direction), (direction) => direction === "next"),
      factorSteps((factor) => run(factor), () => true),
    ],
  }
}

it("puts a setting's presets and steps in one small menu named after it", async () => {
  const run = vi.fn()
  render(<StepsMenus settings={[gainSteps(run)]} />)
  fireEvent.click(screen.getByRole("button", { name: "Gain presets and steps" }))
  expect(await screen.findByRole("menuitem", { name: "Unity" })).toHaveAttribute(
    "aria-disabled",
    "true"
  )
  expect(screen.getByRole("menuitem", { name: "Previous preset" })).toHaveAttribute(
    "aria-disabled",
    "true"
  )
  fireEvent.click(screen.getByRole("menuitem", { name: "Double" }))
  expect(run).toHaveBeenLastCalledWith("double")
  // A step can be taken again without opening the menu again.
  fireEvent.click(screen.getByRole("menuitem", { name: "Next preset" }))
  expect(run).toHaveBeenLastCalledWith("next")
})

it("offers the same entries on the controls' right-click menu, groups apart", () => {
  const items = stepItems(gainSteps())
  expect(
    items.map((item) =>
      typeof item === "object" && "title" in item ? item.title : "—"
    )
  ).toEqual(["Quiet", "Unity", "—", "Previous preset", "Next preset", "—", "Half", "Double"])
})

it("folds a section away from its title and keeps the title in place", () => {
  render(
    <Section title="Loop">
      <p>Loop points</p>
    </Section>
  )
  expect(screen.getByRole("region", { name: "Loop" })).toBeVisible()
  fireEvent.click(screen.getByRole("button", { name: "Fold Loop" }))
  expect(screen.queryByText("Loop points")).toBeNull()
  expect(screen.getByRole("button", { name: "Unfold Loop" })).toHaveAttribute(
    "aria-expanded",
    "false"
  )
  fireEvent.click(screen.getByRole("button", { name: "Unfold Loop" }))
  expect(screen.getByText("Loop points")).toBeVisible()
})
