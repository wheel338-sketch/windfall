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

import { ProjectPlaybackSettings } from "./settings"
import { ProjectInfoPanel } from "./panel"

export function ProjectInfoControls() {
  const settings = useProjectStore((state) => state.project.settings)
  const generation = useProjectGeneration()
  const key = JSON.stringify([
    generation,
    settings.name,
    settings.author,
    settings.genre,
    settings.comments,
  ])

  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="sm">
            Project info
          </Button>
        }
      />
      <PopoverContent
        align="start"
        className="max-h-[75vh] w-96 max-w-[90vw] overflow-y-auto"
      >
        <PopoverHeader>
          <PopoverTitle>Project info</PopoverTitle>
        </PopoverHeader>
        <ProjectPlaybackSettings />
        <ProjectInfoPanel
          key={key}
          settings={settings}
          onSave={async (patch) =>
            Boolean(await dispatch({ type: "updateSettings", patch }))
          }
        />
      </PopoverContent>
    </Popover>
  )
}
