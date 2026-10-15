import { useState } from "react"

import { useTranslation } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { newGestureId } from "@/lib/store/gesture"
import { dispatch, useProjectStore } from "@/lib/store/project"
import {
  getProjectGeneration,
  useProjectGeneration,
} from "@/lib/store/replaced"

import { NotebookPanel } from "./panel"

export function NotebookControls() {
  const t = useTranslation()
  const notebook = useProjectStore((state) => state.project.notebook)
  const generation = useProjectGeneration()
  // The edits of one visit to the notebook are one undo step. Undo or a
  // project save in between starts the next one.
  const [gesture, setGesture] = useState(newGestureId)

  return (
    <Popover
      onOpenChange={(open) => {
        if (open) setGesture(newGestureId())
      }}
    >
      <PopoverTrigger
        render={
          <Button variant="ghost" size="sm">
            {t("Notebook")}
          </Button>
        }
      />
      <PopoverContent
        align="start"
        className="max-h-[75vh] w-96 max-w-[90vw] overflow-y-auto"
      >
        <PopoverHeader>
          <PopoverTitle>{t("Song notebook")}</PopoverTitle>
        </PopoverHeader>
        {/* The panel follows the book itself, so a commit does not start it
            over; only another project does. */}
        <NotebookPanel
          key={generation}
          notebook={notebook}
          onSave={async (book) =>
            // A draft left from the project before is never written into a
            // newly loaded one.
            generation === getProjectGeneration() &&
            Boolean(
              await dispatch({ type: "replaceNotebook", notebook: book }, gesture)
            )
          }
        />
      </PopoverContent>
    </Popover>
  )
}
