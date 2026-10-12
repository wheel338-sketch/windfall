import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { closeProjectOverview, useProjectOverviewPanel } from "./actions"
import { ProjectOverviewView } from "./view"

export function ProjectOverviewPanel() {
  const open = useProjectOverviewPanel((state) => state.open)

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeProjectOverview()
      }}
    >
      <DialogContent className="flex h-dvh max-w-none! flex-col rounded-none">
        <DialogHeader>
          <DialogTitle>Project overview</DialogTitle>
          <DialogDescription>
            Channels, patterns and playlist clips in the open Windfall project.
            Select a relationship to open its piano roll, or a clip to reveal it on the playlist.
          </DialogDescription>
        </DialogHeader>
        {open && <ProjectOverviewView />}
      </DialogContent>
    </Dialog>
  )
}
