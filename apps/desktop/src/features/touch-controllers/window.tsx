import { useRef } from "react"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { Cancel01Icon, MusicNote03Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import type { Channel } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogDescription,
  DialogHeader,
  DialogPortal,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"

import { closeTouchControllers } from "./actions"
import { TouchKeyboard } from "./keyboard"
import { TouchPads } from "./pads"
import { useTouchControllersStore } from "./store"

/** The Select value that follows the channel selected in the rack. */
const FOLLOW_SELECTION = "selected"

/**
 * The touch keyboard and pads, floating over the bottom of the window. It
 * is not modal: the rest of the app stays in reach while it is open, and a
 * press outside does not close it. Closing it unmounts the playing surface,
 * which lets go of every held note.
 */
export function TouchControllersWindow() {
  const open = useTouchControllersStore((state) => state.open)
  const popup = useRef<HTMLDivElement>(null)

  return (
    <Dialog
      open={open}
      modal={false}
      disablePointerDismissal
      onOpenChange={(next, details) => {
        if (next) return
        // Escape belongs to whatever has the focus. It closes this window
        // only from inside it, not while the user works in a panel.
        if (
          details.reason === "escape-key" &&
          !popup.current?.contains(document.activeElement)
        ) {
          details.cancel()
          return
        }
        closeTouchControllers()
      }}
    >
      <DialogPortal>
        <DialogPrimitive.Popup
          ref={popup}
          data-slot="touch-controllers"
          className="fixed bottom-10 left-1/2 z-50 flex max-h-[calc(100%-4rem)] w-[min(44rem,calc(100%-2rem))] -translate-x-1/2 flex-col gap-3 overflow-y-auto rounded-xl bg-popover p-3 text-xs/relaxed text-popover-foreground shadow-lg ring-1 ring-foreground/10 duration-100 outline-none data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        >
          <DialogHeader className="pr-8">
            <DialogTitle>Touch keyboard and pads</DialogTitle>
            <DialogDescription>
              Plays a channel without changing the project. Lower on a key or
              a pad is louder.
            </DialogDescription>
          </DialogHeader>
          {open && <TouchControllers />}
          <DialogClose
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                className="absolute top-2 right-2"
              />
            }
          >
            <HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
            <span className="sr-only">Close</span>
          </DialogClose>
        </DialogPrimitive.Popup>
      </DialogPortal>
    </Dialog>
  )
}

function TouchControllers() {
  const channels = useProjectStore((state) => state.project.channels)
  const selected = useUiStore((state) => state.selectedChannel)
  const override = useTouchControllersStore((state) => state.channelOverride)
  const mode = useTouchControllersStore((state) => state.mode)
  const setMode = useTouchControllersStore((state) => state.setMode)
  const generation = useProjectGeneration()

  const picked = channels.find((channel) => channel.id === override)
  // As in the piano roll: with nothing selected, the first channel.
  const target =
    picked ?? channels.find((channel) => channel.id === selected) ?? channels[0]

  if (!target) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <HugeiconsIcon icon={MusicNote03Icon} strokeWidth={2} />
          </EmptyMedia>
          <EmptyTitle>No channel to play</EmptyTitle>
          <EmptyDescription>
            Add a channel to the channel rack, then play it from here.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  // A surface belongs to one channel of one project. Another target starts
  // it over, which releases what the one before was holding.
  const surface = `${generation}:${target.id}`
  return (
    <Tabs
      value={mode}
      onValueChange={(value) => {
        if (value === "keyboard" || value === "pads") setMode(value)
      }}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <TabsList aria-label="Controller">
          <TabsTrigger value="keyboard">Keyboard</TabsTrigger>
          <TabsTrigger value="pads">Pads</TabsTrigger>
        </TabsList>
        <TargetSelect
          channels={channels}
          target={target}
          following={picked === undefined}
        />
      </div>
      <TabsContent value="keyboard">
        <TouchKeyboard key={surface} channel={target} />
      </TabsContent>
      <TabsContent value="pads">
        <TouchPads key={surface} channel={target} channels={channels} />
      </TabsContent>
    </Tabs>
  )
}

/** Which channel the keyboard and the note pads play. */
function TargetSelect({
  channels,
  target,
  following,
}: {
  channels: readonly Channel[]
  target: Channel
  following: boolean
}) {
  const setOverride = useTouchControllersStore(
    (state) => state.setChannelOverride
  )
  return (
    <Select
      value={following ? FOLLOW_SELECTION : String(target.id)}
      onValueChange={(value) => {
        if (value === FOLLOW_SELECTION) {
          setOverride(null)
          return
        }
        const picked = channels.find((channel) => String(channel.id) === value)
        if (picked) setOverride(picked.id)
      }}
    >
      <SelectTrigger size="sm" aria-label="Target channel">
        <SelectValue>
          {following ? `Selected channel: ${target.name}` : target.name}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectItem value={FOLLOW_SELECTION}>Selected channel</SelectItem>
          {channels.map((channel) => (
            <SelectItem key={channel.id} value={String(channel.id)}>
              {channel.name}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
