import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import { useProjectStore } from "@/lib/store/project"
import { applyUiScale } from "@/lib/ui-scale"

import BrowserPanel from "."
import { findItem, startBrowserTest, tree } from "./testing"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

afterEach(() => {
  cleanup()
  applyUiScale(100)
})

describe.each([75, 100, 125, 200])("browser layout at scale %i", (scale) => {
  it("keeps the virtual tree inside a nonshrinking window, with all tools in scrollable overflow", async () => {
    const app = await startBrowserTest()
    const user = userEvent.setup()
    try {
      act(() => applyUiScale(scale))
      const { container } = render(<BrowserPanel />)
      await findItem("Drums")
      const panel = container.querySelector("[data-slot=browser-panel]")!
      const window = container.querySelector("[data-slot=browser-tree-window]")!
      const scroller = container.querySelector("[data-slot=browser-scroll]")!
      // These assertions lock the flex/overflow contract. jsdom cannot prove
      // pixel height or visibility; measured browser evidence is in UI-SCALING.md.
      expect(panel).toHaveClass(
        "h-full",
        "min-h-0",
        "overflow-auto",
        "*:min-w-54"
      )
      expect(window).toHaveClass("min-h-32", "shrink-0", "flex-1", "flex-col")
      expect(window).toContainElement(scroller as HTMLElement)
      expect(scroller).toHaveClass("overflow-y-auto", "flex-1")
      expect(panel).toContainElement(screen.getByRole("searchbox"))
      expect(panel).toContainElement(
        screen.getByRole("button", { name: "Add folder…" })
      )
      expect(panel).toContainElement(
        screen.getByRole("button", { name: "Browse plugins" })
      )
      expect(panel).toContainElement(
        screen.getByRole("region", { name: "Preview" })
      )

      const project = useProjectStore.getState().project
      const history = useProjectStore.getState().history
      await user.click(
        screen.getByRole("button", { name: "Preview sounds when selected" })
      )
      await user.type(screen.getByRole("searchbox"), '"kick 02"')
      await user.click(await findItem("Kick 02.wav"))
      expect(tree()).toHaveAttribute(
        "aria-activedescendant",
        screen.getByRole("treeitem", { selected: true }).id
      )
      expect(await screen.findByText(/44.1 kHz/)).toBeInTheDocument()
      await user.click(await screen.findByRole("button", { name: "Edit tags" }))
      const tags = await screen.findByRole("textbox", { name: "Tags" })
      await waitFor(() => expect(tags).toBeEnabled())
      expect(panel).toContainElement(tags)
      expect(panel).toContainElement(
        screen.getByRole("button", { name: "Star selected file" })
      )
      await user.type(tags, "scaled")
      await user.click(screen.getByRole("button", { name: "Save tags" }))
      expect(
        await screen.findByText("scaled", { selector: "[data-slot=badge]" })
      ).toBeInTheDocument()
      expect(screen.getByRole("button", { name: "Play preview" })).toBeEnabled()
      expect(screen.getByRole("button", { name: "Add to rack" })).toBeEnabled()
      expect(
        screen.getByRole("button", { name: "Add to playlist" })
      ).toBeEnabled()
      expect(useProjectStore.getState().project).toBe(project)
      expect(useProjectStore.getState().history).toBe(history)
    } finally {
      app.stop()
    }
  })
})
