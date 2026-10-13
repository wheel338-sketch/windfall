import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { closeProjectOverview, useProjectOverviewPanel } from "./actions"
import { ProjectOverviewView } from "./view"
import { translatedText as t } from "@/lib/i18n"

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
          <DialogTitle>{t("Project overview")}</DialogTitle>
          <DialogDescription>
            {t("Channels, patterns and playlist clips in the open Windfall project. Select a relationship to open its piano roll, or a clip to reveal it on the playlist.")}
          </DialogDescription>
        </DialogHeader>
        {open && <ProjectOverviewView />}
      </DialogContent>
    </Dialog>
  )
}
