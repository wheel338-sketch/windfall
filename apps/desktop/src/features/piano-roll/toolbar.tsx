import type { ReactNode, Ref } from "react"
import {
  ArrowRightDoubleIcon,
  Cursor01Icon,
  Eraser01Icon,
  FitToScreenIcon,
  GhostIcon,
  PaintBrush01Icon,
  PencilEdit02Icon,
  VolumeMute02Icon,
  ScissorIcon,
  PlayIcon,
  ZoomInAreaIcon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon, type IconSvgElement } from "@hugeicons/react"

import type { ChannelId } from "@/bindings"
import { ActionButton } from "@/components/action-button"
import { NumberField } from "@/components/audio/number-field"
import { ContextActions } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  isChecked,
  runAction,
  useAction,
  useActionEnabled,
  useAppSelector,
  useShortcutLabel,
} from "@/lib/actions"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { colorToCss } from "@/lib/units"
import { isNoteArticulation, NOTE_ARTICULATIONS } from "@/lib/note-expression"

import { nextDrawArticulation } from "./articulation-step"
import { useSession } from "./context"
import { nextDrawColor } from "./draw-color-step"
import { nextDrawGlideScale } from "./draw-glide-scale"
import { VIEW_MENU } from "./menu"
import { nextPianoTool } from "./piano-tool-step"
import { isSnapId, SNAP_OPTIONS, type SnapOption } from "./snap"
import { nextSnapScale } from "./snap-scale"
import { usePianoRollStore } from "./store"
import { ScaleControls } from "./scale-controls"
import { StampMenu } from "./stamp-menu"
import { NoteColorPicker } from "./note-color-picker"
import { WaveformHelperControl } from "./waveform-helper"
import { ChordToolsControl } from "./chords/control"
import { ChordReadout } from "./chords/readout-control"
import { ScaleHighlightControl } from "./scale/control"
import { useTypingKeyboardStore } from "./typing-keyboard"
import { useStepEntryStore } from "./step-entry"
import { message, translatedText as t, useActionTitle } from "@/lib/i18n"

type RollButtonProps = {
  action: string
  icon: IconSvgElement
}

/**
 * A toolbar button for a piano roll action. Like the shell's action button,
 * but it lights up while its action is on and hands the keyboard back to
 * the notes after a click.
 */
function RollButton({ action: id, icon }: RollButtonProps) {
  const session = useSession()
  const action = useAction(id)
  const enabled = useActionEnabled(id)
  const checked = useAppSelector((state) =>
    action ? isChecked(action, state) : false
  )
  const shortcut = useShortcutLabel(id)
  const title = useActionTitle(id, action?.title ?? "")
  const hint = useHint(shortcut ? message("{help} ({shortcut})", { help: title, shortcut }) : title)

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant={checked ? "secondary" : "ghost"}
            size="icon-sm"
            aria-label={title}
            aria-pressed={action?.checked ? checked : undefined}
            disabled={!enabled}
            className={checked ? "text-brand" : undefined}
            onClick={() => {
              void runAction(id)
              session.focusGrid()
            }}
            {...hint}
          />
        }
      >
        <HugeiconsIcon icon={icon} strokeWidth={2} />
      </TooltipTrigger>
      <TooltipContent side="bottom">
        {title}
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}

function Group({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div role="group" aria-label={label} className="flex items-center gap-0.5">
      {children}
    </div>
  )
}

function ChannelPicker({ channelId }: { channelId: ChannelId }) {
  const session = useSession()
  const channels = useProjectStore((state) => state.project.channels)
  const selectChannel = useUiStore((state) => state.selectChannel)
  const hint = useHint(t("The channel whose notes are being edited"))
  const items = channels.map((channel) => ({
    value: channel.id,
    label: channel.name,
  }))

  return (
    <Select
      items={items}
      value={channelId}
      onValueChange={(next: ChannelId | null) => {
        if (next !== null) selectChannel(next)
        session.focusGrid()
      }}
    >
      <SelectTrigger
        size="sm"
        aria-label={t("Channel")}
        className="max-w-44 min-w-28"
        {...hint}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent alignItemWithTrigger={false} align="start">
        <SelectGroup>
          {channels.map((channel) => (
            <SelectItem key={channel.id} value={channel.id}>
              <span
                aria-hidden
                className="size-2 shrink-0 rounded-[2px]"
                style={{ background: colorToCss(channel.color) }}
              />
              <span className="truncate">{channel.name}</span>
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

const SNAP_GROUPS: SnapOption["group"][] = ["none", "step", "beat", "bar"]

function SnapPicker() {
  const session = useSession()
  const snap = usePianoRollStore((state) => state.snap)
  const setSnap = usePianoRollStore((state) => state.setSnap)
  const hint = useHint(
    t("Snap: the grid that drawing, moving and resizing notes stick to. Hold Alt while dragging to ignore it")
  )
  const items = SNAP_OPTIONS.map((option) => ({
    value: option.id,
    label: option.label,
  }))

  return (
    <label className="flex items-center gap-1.5">
      <span className="text-muted-foreground">{t("Snap")}</span>
      <Select
        items={items}
        value={snap}
        onValueChange={(next: string | null) => {
          if (isSnapId(next)) setSnap(next)
          session.focusGrid()
        }}
      >
        <SelectTrigger size="sm" aria-label={t("Snap")} className="w-24" {...hint}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false} align="start">
          {SNAP_GROUPS.map((group, index) => (
            <SelectGroup key={group}>
              {index > 0 && <SelectSeparator />}
              {SNAP_OPTIONS.filter((option) => option.group === group).map(
                (option) => (
                  <SelectItem key={option.id} value={option.id}>
                    {option.label}
                  </SelectItem>
                )
              )}
            </SelectGroup>
          ))}
        </SelectContent>
      </Select>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose a finer snap")}
        disabled={nextSnapScale(snap, "finer") === null}
        onClick={() => {
          const next = nextSnapScale(usePianoRollStore.getState().snap, "finer")
          if (next !== null) usePianoRollStore.getState().setSnap(next)
        }}
      >
        {t("Finer")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose a coarser snap")}
        disabled={nextSnapScale(snap, "coarser") === null}
        onClick={() => {
          const next = nextSnapScale(usePianoRollStore.getState().snap, "coarser")
          if (next !== null) usePianoRollStore.getState().setSnap(next)
        }}
      >
        {t("Coarser")}
      </Button>
    </label>
  )
}

type ToolbarProps = {
  channelId: ChannelId
  /** Filled by the panel with the position under the pointer. */
  readoutRef: Ref<HTMLOutputElement>
}

function ArticulationPicker() {
  const session = useSession()
  const articulation = usePianoRollStore((state) => state.drawArticulation)
  const glideTicks = usePianoRollStore((state) => state.drawGlideTicks)
  const setArticulation = usePianoRollStore((state) => state.setDrawArticulation)
  const setGlideTicks = usePianoRollStore((state) => state.setDrawGlideTicks)
  return (
    <Group label={t("New note articulation")}>
      <Select items={NOTE_ARTICULATIONS} value={articulation} onValueChange={(next) => {
        if (isNoteArticulation(next)) setArticulation(next)
        session.focusGrid()
      }}>
        <SelectTrigger size="sm" aria-label={t("New note articulation")} className="w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false} align="start">
          <SelectGroup>
            {NOTE_ARTICULATIONS.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}
          </SelectGroup>
        </SelectContent>
      </Select>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose the previous articulation for new notes")}
        disabled={nextDrawArticulation(articulation, "previous") === null}
        onClick={() => {
          const latest = usePianoRollStore.getState().drawArticulation
          const next = nextDrawArticulation(latest, "previous")
          if (next !== null) setArticulation(next)
        }}
      >
        {t("Previous")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose the next articulation for new notes")}
        disabled={nextDrawArticulation(articulation, "next") === null}
        onClick={() => {
          const latest = usePianoRollStore.getState().drawArticulation
          const next = nextDrawArticulation(latest, "next")
          if (next !== null) setArticulation(next)
        }}
      >
        {t("Next")}
      </Button>
      {articulation === "portamento" && (
        <>
          <NumberField
            size="sm" aria-label={t("Portamento duration")} value={glideTicks}
            onValueChange={setGlideTicks} min={1} max={245760} step={1}
            coarseStep={60} unit={t("ticks")} className="w-24"
          />
          <Button
            variant="outline"
            size="sm"
            aria-label={t("Halve the duration for new portamento notes")}
            disabled={nextDrawGlideScale(glideTicks, "half") === null}
            onClick={() => {
              const next = nextDrawGlideScale(
                usePianoRollStore.getState().drawGlideTicks,
                "half"
              )
              if (next !== null) usePianoRollStore.getState().setDrawGlideTicks(next)
            }}
          >
            {t("Halve")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            aria-label={t("Double the duration for new portamento notes")}
            disabled={nextDrawGlideScale(glideTicks, "double") === null}
            onClick={() => {
              const next = nextDrawGlideScale(
                usePianoRollStore.getState().drawGlideTicks,
                "double"
              )
              if (next !== null) usePianoRollStore.getState().setDrawGlideTicks(next)
            }}
          >
            {t("Double")}
          </Button>
        </>
      )}
    </Group>
  )
}

function DrawColorPicker() {
  const session = useSession()
  const color = usePianoRollStore((state) => state.drawColorGroup)
  const setColor = usePianoRollStore((state) => state.setDrawColorGroup)
  return (
    <>
      <NoteColorPicker compact value={color} onChange={(value) => {
        if (value !== "keep") setColor(value)
        session.focusGrid()
      }} />
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose the previous draw color")}
        disabled={nextDrawColor(color, "previous") === null}
        onClick={() => {
          const latest = usePianoRollStore.getState().drawColorGroup
          const next = nextDrawColor(latest, "previous")
          if (next !== null) setColor(next.color)
        }}
      >
        {t("Previous color")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose the next draw color")}
        disabled={nextDrawColor(color, "next") === null}
        onClick={() => {
          const latest = usePianoRollStore.getState().drawColorGroup
          const next = nextDrawColor(latest, "next")
          if (next !== null) setColor(next.color)
        }}
      >
        {t("Next color")}
      </Button>
    </>
  )
}

/** The strip above the grid: channel, tools, snap, view switches, readout. */
export function PianoRollToolbar({ channelId, readoutRef }: ToolbarProps) {
  const tool = usePianoRollStore((state) => state.tool)
  const setTool = usePianoRollStore((state) => state.setTool)
  const drum = usePianoRollStore((state) => state.drum)
  const typing = useTypingKeyboardStore((state) => state.enabled)
  const stepEntry = useStepEntryStore((state) => state.enabled)
  const readoutHint = useHint(
    t("Position under the pointer as bar, beat and tick, and the key")
  )

  return (
    <ContextActions items={VIEW_MENU}>
      <div
        role="toolbar"
        aria-label={t("Piano roll")}
        className="flex h-9 shrink-0 items-center gap-3 overflow-x-auto overflow-y-hidden border-b bg-chassis/40 px-1.5 whitespace-nowrap"
      >
        <ChannelPicker channelId={channelId} />
        <ActionButton
          action="pianoRoll.typing"
          variant={typing ? "secondary" : "ghost"}
          size="sm"
          aria-pressed={typing}
          className={typing ? "text-brand" : undefined}
        />
        <ActionButton
          action="pianoRoll.stepEntry"
          variant={stepEntry ? "secondary" : "ghost"}
          size="sm"
          aria-pressed={stepEntry}
          className={stepEntry ? "text-brand" : undefined}
        />
        <Group label={t("Tools")}>
          <RollButton action="pianoRoll.toolDraw" icon={PencilEdit02Icon} />
          <RollButton action="pianoRoll.toolPaint" icon={PaintBrush01Icon} />
          <RollButton action="pianoRoll.toolSelect" icon={Cursor01Icon} />
          <RollButton action="pianoRoll.toolErase" icon={Eraser01Icon} />
          <RollButton action="pianoRoll.toolMute" icon={VolumeMute02Icon} />
          <RollButton action="pianoRoll.toolSlice" icon={ScissorIcon} />
          <RollButton action="pianoRoll.toolZoom" icon={ZoomInAreaIcon} />
          <RollButton action="pianoRoll.toolPlayback" icon={PlayIcon} />
        </Group>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={t("Choose the previous piano tool")}
          disabled={nextPianoTool(tool, "previous") === null}
          onClick={() => {
            const latest = usePianoRollStore.getState().tool
            const next = nextPianoTool(latest, "previous")
            if (next !== null) setTool(next)
          }}
        >
          {t("Previous")}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={t("Choose the next piano tool")}
          disabled={nextPianoTool(tool, "next") === null}
          onClick={() => {
            const latest = usePianoRollStore.getState().tool
            const next = nextPianoTool(latest, "next")
            if (next !== null) setTool(next)
          }}
        >
          {t("Next")}
        </Button>
        <ActionButton
          action="pianoRoll.drum"
          variant={drum ? "secondary" : "ghost"}
          size="sm"
          aria-pressed={drum}
          className={drum ? "text-brand" : undefined}
        >
          {t("Drum")}
        </ActionButton>
        <SnapPicker />
        <ArticulationPicker />
        <DrawColorPicker />
        <ScaleControls />
        <StampMenu />
        <ChordToolsControl />
        <ChordReadout />
        <ActionButton action="pianoRoll.generateRiff" variant="outline" size="sm" />
        <ScaleHighlightControl />
        <WaveformHelperControl />
        <ActionButton action="pianoRoll.quantize" variant="outline" size="sm">
          {t("Note tools")}
        </ActionButton>
        <Group label={t("View")}>
          <RollButton action="pianoRoll.ghosts" icon={GhostIcon} />
          <ActionButton action="pianoRoll.editGhosts" variant="ghost" size="sm">{t("Edit ghosts")}</ActionButton>
          <RollButton action="pianoRoll.follow" icon={ArrowRightDoubleIcon} />
          <RollButton action="pianoRoll.zoomFit" icon={FitToScreenIcon} />
          <RollButton action="pianoRoll.zoomSelection" icon={ZoomInAreaIcon} />
        </Group>
        <output
          ref={readoutRef}
          aria-label={t("Position under the pointer")}
          className="ml-auto flex h-6 min-w-36 shrink-0 items-center justify-end rounded-sm bg-display px-2 font-readout text-[0.6875rem] whitespace-pre text-display-foreground empty:before:text-display-dim empty:before:content-['–.–.–––']"
          {...readoutHint}
        />
      </div>
    </ContextActions>
  )
}
