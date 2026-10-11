import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { usePlaylistStore } from "../store"
import { arrangementCommand } from "./commands"
import { emptyArrangementBook } from "./model"
import { ArrangementPanel } from "./panel"
import { activeArrangementClipIds } from "./select-clips"

const EMPTY_BOOK = emptyArrangementBook()

export function ArrangementControls() {
  const project = useProjectStore((state) => state.project)
  const book = project.playlist.arrangementBook ?? EMPTY_BOOK
  const clips = project.playlist.clips.map((clip) => {
    const content = clip.content
    const name =
      content.type === "pattern"
        ? (project.patterns.find((pattern) => pattern.id === content.pattern)
            ?.name ?? `Pattern clip ${clip.id}`)
        : content.type === "audio"
          ? (project.samples.find((sample) => sample.id === content.sample)
              ?.name ?? `Audio clip ${clip.id}`)
          : (project.automations.find(
              (automation) => automation.id === content.automation
            )?.name ?? `Automation clip ${clip.id}`)
    return { id: clip.id, name }
  })
  const arrangementClipIds = activeArrangementClipIds(
    book,
    clips.map((clip) => clip.id)
  )
  const send = async (command: Parameters<typeof dispatch>[0]) => {
    if (!(await dispatch(command)))
      throw new Error("The arrangement edit was not applied.")
  }
  return (
    <div className="flex shrink-0 items-center gap-1 border-b px-2 py-1">
      <Popover>
        <PopoverTrigger
          render={
            <Button variant="ghost" size="sm">
              Arrangements
              {book.arrangements.length ? ` (${book.arrangements.length})` : ""}
            </Button>
          }
        />
        <PopoverContent
          align="start"
          className="max-h-[75vh] w-96 max-w-[90vw] overflow-y-auto"
        >
          <ArrangementPanel
            value={book}
            tracks={project.playlist.tracks}
            clipIds={clips.map((clip) => clip.id)}
            clips={clips}
            channels={project.channels}
            sources={project.samples}
            onEdit={(edit) => send(arrangementCommand(edit))}
            onAddLayout={(name, copy) =>
              send({ type: "addArrangementLayout", name, copy })
            }
            makeUniqueClipIds={project.playlist.clips.map((clip) => clip.id)}
            onMakeUnique={(clip) => send({ type: "makeUnique", clip })}
          />
        </PopoverContent>
      </Popover>
      <Button
        variant="ghost"
        size="sm"
        title="Select arrangement clips"
        disabled={arrangementClipIds === null}
        onClick={() => {
          if (arrangementClipIds !== null)
            usePlaylistStore.getState().select(arrangementClipIds)
        }}
      >
        Select arrangement clips
      </Button>
    </div>
  )
}
