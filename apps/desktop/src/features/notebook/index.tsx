import { useTranslation } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"

import { NotebookPanel } from "./panel"

export function NotebookControls() {
  const t = useTranslation()
  const notebook = useProjectStore((state) => state.project.notebook)
  const generation = useProjectGeneration()

  return (
    <Popover>
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
        <NotebookPanel
          key={JSON.stringify([generation, notebook])}
          notebook={notebook}
          onSave={async (book) =>
            Boolean(await dispatch({ type: "replaceNotebook", notebook: book }))
          }
        />
      </PopoverContent>
    </Popover>
  )
}
