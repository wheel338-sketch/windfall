import { create } from "zustand"

import { registry, type Action } from "@/lib/actions"
import { dynamicTitle } from "@/lib/i18n"

export const useProjectOverviewPanel = create(() => ({ open: false }))

export function closeProjectOverview() {
  useProjectOverviewPanel.setState({ open: false })
}

const PROJECT_OVERVIEW_ACTIONS: Action[] = [
  {
    id: "view.projectOverview",
    ...dynamicTitle("Show project overview", {}),
    section: "View",
    keywords: "channels patterns clips relationships",
    run: () => useProjectOverviewPanel.setState({ open: true }),
  },
]

export function registerProjectOverviewActions(): () => void {
  const unregister = registry.register(PROJECT_OVERVIEW_ACTIONS)
  return () => {
    unregister()
    closeProjectOverview()
  }
}
