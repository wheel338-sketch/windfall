import { useEffect, useRef, type ReactNode } from "react"

import type { TrackId } from "@/bindings"
import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { EFFECT_KINDS } from "@/features/params"
import { useHint, useUiStore } from "@/lib/store"
import { colorToCss } from "@/lib/units"
import { cn } from "@/lib/utils"

import { TRACK_COLORS } from "./colors"
import { addEffectActionId } from "./effect-actions"
import { useMixerUi } from "./mixer-ui"
import { renameTrack, setTrackColor, startRename } from "./operations"
import { RecordArm } from "./recording-input"
import { translatedText as t } from "@/lib/i18n"

/** The effects that can be added to the selected track. */
const ADD_EFFECT: ContextItem = {
  get submenu() { return t("Add effect") },
  items: EFFECT_KINDS.map(addEffectActionId),
}

const INSERT_MENU: ContextItem[] = [
  "mixer.renderSelected",
  "mixer.renderArmed",
  contextSeparator,
  "mixer.toggleRecordArm",
  "mixer.recordArmed",
  contextSeparator,
  "mixer.renameTrack",
  "mixer.changeColor",
  contextSeparator,
  ADD_EFFECT,
  "mixer.effects",
  "mixer.bypassEffects",
  "mixer.enableEffects",
  contextSeparator,
  "mixer.toggleMute",
  "mixer.toggleSolo",
  "mixer.unmuteAll",
  "mixer.resetLevels",
  "mixer.selectRoutedHere",
  "mixer.unsoloAll",
  contextSeparator,
  "mixer.resetVolume",
  "mixer.centerPan",
  "mixer.routeToMaster",
  contextSeparator,
  "mixer.addTrack",
  "mixer.deleteTrack",
]

const MASTER_MENU: ContextItem[] = [
  "mixer.renderSelected",
  "mixer.renderArmed",
  contextSeparator,
  "mixer.toggleRecordArm",
  "mixer.recordArmed",
  contextSeparator,
  "mixer.renameTrack",
  "mixer.changeColor",
  contextSeparator,
  ADD_EFFECT,
  "mixer.effects",
  "mixer.bypassEffects",
  "mixer.enableEffects",
  contextSeparator,
  "mixer.toggleMute",
  "mixer.unmuteAll",
  "mixer.resetLevels",
  "mixer.selectRoutedHere",
  "mixer.unsoloAll",
  contextSeparator,
  "mixer.resetVolume",
  "mixer.centerPan",
  contextSeparator,
  "mixer.addTrack",
]

/** How long after it opens the name field takes the focus back. */
const FOCUS_GRACE_MS = 250

function NameField({ id, name }: { id: TrackId; name: string }) {
  const field = useRef<HTMLInputElement>(null)
  const opened = useRef(0)
  const closed = useRef(false)

  useEffect(() => {
    opened.current = performance.now()
    field.current?.focus()
    field.current?.select()
  }, [])

  function close(save: boolean) {
    if (closed.current) return
    closed.current = true
    const typed = field.current?.value ?? name
    useMixerUi.setState({ renaming: null })
    if (save) void renameTrack(id, typed)
  }

  return (
    <input
      ref={field}
      defaultValue={name}
      aria-label={t("Track name")}
      maxLength={64}
      spellCheck={false}
      className="h-4 min-w-0 flex-1 rounded-sm bg-background px-1 text-[11px] font-medium text-foreground ring-1 ring-ring outline-none"
      onDoubleClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== "Escape") return
        event.preventDefault()
        const strip = event.currentTarget.closest<HTMLElement>("[data-track]")
        close(event.key === "Enter")
        strip?.focus({ preventScroll: true })
      }}
      onBlur={() => {
        // A menu that is closing hands the focus back to where it was,
        // which would end the rename the menu just started.
        if (performance.now() - opened.current < FOCUS_GRACE_MS) {
          requestAnimationFrame(() => {
            if (!closed.current) field.current?.focus()
          })
          return
        }
        close(true)
      }}
    />
  )
}

function Swatches({ id, color }: { id: TrackId; color: number }) {
  return (
    <div
      role="group"
      aria-label={t("Track color")}
      className="grid grid-cols-7 gap-1"
    >
      {TRACK_COLORS.map((swatch) => (
        <button
          key={swatch.color}
          type="button"
          aria-label={swatch.name}
          aria-pressed={swatch.color === color}
          title={swatch.name}
          className="size-5 rounded-sm ring-offset-2 ring-offset-popover outline-none hover:brightness-110 focus-visible:ring-2 focus-visible:ring-ring aria-pressed:ring-2 aria-pressed:ring-foreground"
          style={{ backgroundColor: colorToCss(swatch.color) }}
          onClick={() => {
            useMixerUi.setState({ coloring: null })
            void setTrackColor(id, swatch.color)
          }}
        />
      ))}
    </div>
  )
}

/** The bar in the track color along the top. Click it to pick another. */
function ColorBar({ id, color }: { id: TrackId; color: number }) {
  const open = useMixerUi((state) => state.coloring === id)
  const hint = useHint(t("Track color. Click to change it"))
  return (
    <Popover
      open={open}
      onOpenChange={(next) =>
        useMixerUi.setState({ coloring: next ? id : null })
      }
    >
      <PopoverTrigger
        render={
          <button
            type="button"
            tabIndex={-1}
            aria-label={t("Change track color")}
            data-slot="track-color"
            className="block h-1.5 w-full shrink-0 outline-none hover:brightness-125 focus-visible:ring-2 focus-visible:ring-foreground focus-visible:ring-inset"
            style={{ backgroundColor: colorToCss(color) }}
            {...hint}
          />
        }
      />
      <PopoverContent side="bottom" align="start" className="w-auto p-2">
        <Swatches id={id} color={color} />
      </PopoverContent>
    </Popover>
  )
}

type StripHeaderProps = {
  id: TrackId
  name: string
  color: number
  /** The track's place in the mixer, or "M" for the master. */
  number: string
  master: boolean
  /** A control for the right end of the name row. */
  trailing?: ReactNode
}

/**
 * The top of a strip: color bar, number and name. Double-click renames in
 * place, and a right-click lists everything that can be done to the track.
 */
export function StripHeader({
  id,
  name,
  color,
  number,
  master,
  trailing,
}: StripHeaderProps) {
  const renaming = useMixerUi((state) => state.renaming === id)
  const hint = useHint(
    t("Double-click or press F2 to rename. Right-click for more")
  )

  return (
    <ContextActions items={master ? MASTER_MENU : INSERT_MENU}>
      <div
        data-slot="strip-header"
        className="shrink-0"
        // The menu's entries act on the selected track.
        onContextMenu={() => useUiStore.getState().selectTrack(id)}
      >
        <ColorBar id={id} color={color} />
        <div
          className="flex h-5 items-center gap-1 pr-0.5 pl-1.5 group-data-[mode=mini]/strip:h-4"
          onDoubleClick={() => startRename(id)}
          {...hint}
        >
          <span
            className={cn(
              "shrink-0 font-readout text-[9px] text-muted-foreground",
              master && "text-foreground/80"
            )}
          >
            {number}
          </span>
          {renaming ? (
            <NameField id={id} name={name} />
          ) : (
            <span
              data-slot="strip-name"
              title={name}
              className="min-w-0 flex-1 truncate text-[11px] leading-none font-medium group-data-[audible=muted]/strip:text-muted-foreground group-data-[audible=silenced]/strip:text-muted-foreground"
            >
              {name}
            </span>
          )}
          {trailing}
          <RecordArm track={id} />
        </div>
      </div>
    </ContextActions>
  )
}
