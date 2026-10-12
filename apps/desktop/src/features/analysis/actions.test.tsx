import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"
import { registerAllActions } from "@/features/layout/register-actions"
import {
  CommandPalette,
  searchActions,
} from "@/features/palette/command-palette"
import {
  currentKeymap,
  getAppState,
  installKeymap,
  registry,
} from "@/lib/actions"
import { ActionMenuItem } from "@/components/action-menu-item"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { usePlaylistStore } from "@/features/playlist/store"

// This application-wide action/palette/menu contract is not a latency budget.
// Parallel jsdom suites can exceed Vitest's default 5 s while all assertions pass.
it("registers Analysis at startup without an inspector and removes its actions and subscriptions", async () => {
  const unregister = registerAllActions()
  const uninstall = installKeymap()
  try {
    expect(
      searchActions(registry.list(), "analysis").map((action) => action.id)
    ).toContain("analysis.open")
    expect(
      registry.list().filter((action) => action.section === "Analysis")
    ).toHaveLength(15)
    expect(registry.get("analysis.remix")?.title).toBe("Remix a song")
    expect(currentKeymap().byScope.get("global")?.get("Ctrl+Shift+A")).toBe(
      "analysis.open"
    )
    const user = userEvent.setup()
    render(<CommandPalette />)
    await user.keyboard("{Control>}k{/Control}")
    const entry = await screen.findByRole("option", { name: /^Analysis/ })
    expect(entry).toHaveAttribute("aria-disabled", "true")
    expect(entry).toHaveTextContent("Select one audio clip with a source")
    await user.keyboard("{Escape}")
    const action = registry.get("analysis.open")
    if (!action) throw new Error("Analysis action missing")
    render(
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button />}>
          Analysis menu
        </DropdownMenuTrigger>
        <DropdownMenuContent>
          <ActionMenuItem action={action} state={getAppState()} />
        </DropdownMenuContent>
      </DropdownMenu>
    )
    await user.click(screen.getByRole("button", { name: "Analysis menu" }))
    const menu = await screen.findByRole("menuitem", { name: /Analysis/ })
    expect(within(menu).getByText(action.title)).toBeVisible()
    expect(menu).toHaveAttribute("aria-disabled", "true")
  } finally {
    uninstall()
    unregister()
  }
  await waitFor(() => expect(registry.get("analysis.open")).toBeUndefined())
  const version = registry.stateVersion()
  usePlaylistStore.getState().select([71])
  expect(registry.stateVersion()).toBe(version)
}, 10_000)
