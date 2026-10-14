import {
  ArrowDown01Icon,
  ArrowHorizontalIcon,
  ArrowVerticalIcon,
  CursorRectangleSelection01Icon,
  Eraser01Icon,
  FitToScreenIcon,
  LeftToRightListBulletIcon,
  Magnet01Icon,
  Navigation03Icon,
  PaintBrush01Icon,
  PencilEdit01Icon,
  PlayIcon,
  RepeatIcon,
  ScissorIcon,
  VolumeMute01Icon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon, type IconSvgElement } from "@hugeicons/react"

import { ContextActions } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Kbd } from "@/components/ui/kbd"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { ModeSwitch } from "@/features/transport/mode-switch"
import { runAction, useAction, useShortcutLabel } from "@/lib/actions"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { cn } from "@/lib/utils"

import { toolActionId } from "./actions"
import { TOOLS, type Tool } from "./intents"
import { OverlapWarning } from "./audio/overlap-warning"
import { PANEL_MENU } from "./menu"
import { useViewportValue, type GridMetrics } from "./metrics"
import { nextPlaylistSnapScale } from "./playlist-snap-scale"
import { nextPlaylistTool } from "./playlist-tool-step"
import { SNAP_MODES, type SnapMode } from "./snap"
import { usePlaylistStore } from "./store"
import { translatedText as t, message, useActionTitle } from "@/lib/i18n"

const TOOL_ICONS: Record<Tool, IconSvgElement> = {
  draw: PencilEdit01Icon,
  paint: PaintBrush01Icon,
  select: CursorRectangleSelection01Icon,
  erase: Eraser01Icon,
  mute: VolumeMute01Icon,
  slip: ArrowHorizontalIcon,
  playback: PlayIcon,
  slice: ScissorIcon,
}

const TOOL_ABOUT: Record<Tool, string> = {
  get draw() { return t("click to place what is picked on the left, drag clips to move or resize, click in an automation clip to add a point") },
  get paint() { return t("drag to lay what is picked on the left down back to back") },
  get select() { return t("drag a box around clips, then move, copy or delete them") },
  get erase() { return t("click or drag across clips to delete them") },
  get mute() { return t("click or drag across clips to mute or unmute them") },
  get slip() { return t("drag a clip's content left or right inside its fixed start and end") },
  get playback() { return t("click or drag to seek the transport on the playlist grid") },
  get slice() { return t("drag a vertical line, then release to split every clip crossing it") },
}

type IconActionProps = {
  /** Id of the registry action the button runs. */
  action: string
  icon: IconSvgElement
  /** For an action that is on or off. */
  pressed?: boolean
  /** Added to the status bar hint after the title. */
  about?: string
  className?: string
}

/**
 * A toolbar button for one of the playlist's actions, for the ones that
 * are on or off or need more of a hint than their title.
 */
function IconAction({
  action: id,
  icon,
  pressed,
  about,
  className,
}: IconActionProps) {
  const action = useAction(id)
  const shortcut = useShortcutLabel(id)
  const title = useActionTitle(id, action?.title ?? "").replace(/…$/, "")
  const named = shortcut ? message("{help} ({shortcut})", { help: title, shortcut }) : title
  const hint = useHint(about ? message("{name}: {value}", { name: named, value: about }) : named)

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant={pressed ? "secondary" : "ghost"}
            size="icon-sm"
            aria-label={title}
            aria-pressed={pressed}
            className={cn(
              "text-muted-foreground aria-pressed:text-foreground",
              className
            )}
            onClick={() => void runAction(id)}
            {...hint}
          />
        }
      >
        <HugeiconsIcon icon={icon} strokeWidth={2} className="size-3.5" />
      </TooltipTrigger>
      <TooltipContent side="bottom">
        {title}
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  )
}

function Tools() {
  const tool = usePlaylistStore((state) => state.tool)

  function stepTool(direction: "previous" | "next") {
    const latest = usePlaylistStore.getState().tool
    const next = nextPlaylistTool(latest, direction)
    if (next !== null) usePlaylistStore.getState().setTool(next)
  }

  return (
    <div className="flex items-center gap-1.5">
      <div
        role="group"
        aria-label={t("Tools")}
        className="flex items-center rounded-md bg-(--wf-step-off)/60 p-px"
      >
        {TOOLS.map((item) => (
          <IconAction
            key={item}
            action={toolActionId(item)}
            icon={TOOL_ICONS[item]}
            pressed={item === tool}
            about={TOOL_ABOUT[item]}
            className="aria-pressed:bg-background aria-pressed:shadow-xs"
          />
        ))}
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose the previous playlist tool")}
        disabled={nextPlaylistTool(tool, "previous") === null}
        onClick={() => stepTool("previous")}
      >
        {t("Previous")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose the next playlist tool")}
        disabled={nextPlaylistTool(tool, "next") === null}
        onClick={() => stepTool("next")}
      >
        {t("Next")}
      </Button>
    </div>
  )
}

function StepToggle() {
  const step = usePlaylistStore((state) => state.step)
  const shortcut = useShortcutLabel("playlist.step")
  const hint = useHint(
    shortcut
      ? message("Step ({shortcut}): drag across an automation curve to write held points on the snap grid", { shortcut })
      : t("Step: drag across an automation curve to write held points on the snap grid")
  )
  return (
    <Button
      variant={step ? "secondary" : "ghost"}
      size="sm"
      aria-label={t("Step")}
      aria-pressed={step}
      onClick={() => void runAction("playlist.step")}
      {...hint}
    >
      {t("Step")}
    </Button>
  )
}

function SnapMenu() {
  const snap = usePlaylistStore((state) => state.snap)
  const setSnap = usePlaylistStore((state) => state.setSnap)
  const current = SNAP_MODES.find((item) => item.mode === snap)
  const hint = useHint(
    t("Snap: what clips line up with when placed, moved and resized. Hold Alt to ignore it for one drag")
  )

  function scaleSnap(direction: "finer" | "coarser") {
    const latest = usePlaylistStore.getState().snap
    const next = nextPlaylistSnapScale(latest, direction)
    if (next !== null) usePlaylistStore.getState().setSnap(next)
  }

  return (
    <div className="flex items-center gap-1.5">
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="outline"
              size="sm"
              aria-label={message("Snap: {currentLabel}", { currentLabel: current?.label ?? t("Snap") })}
              className="gap-1.5 px-1.5"
              {...hint}
            />
          }
        >
          <HugeiconsIcon
            icon={Magnet01Icon}
            strokeWidth={2}
            className={snap === "none" ? "text-muted-foreground" : "text-brand"}
          />
          <span className="w-7 text-left">{current?.label}</span>
          <HugeiconsIcon
            icon={ArrowDown01Icon}
            strokeWidth={2}
            className="text-muted-foreground"
          />
        </DropdownMenuTrigger>
        <DropdownMenuContent className="w-56">
          <DropdownMenuGroup>
            <DropdownMenuLabel>{t("Snap")}</DropdownMenuLabel>
            <DropdownMenuRadioGroup
              value={snap}
              onValueChange={(mode: SnapMode) => setSnap(mode)}
            >
              {SNAP_MODES.map((item) => (
                <DropdownMenuRadioItem
                  key={item.mode}
                  value={item.mode}
                  closeOnClick
                >
                  <span>{item.label}</span>
                  <span className="ml-auto pl-3 text-muted-foreground">
                    {item.about}
                  </span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose a finer playlist snap")}
        disabled={nextPlaylistSnapScale(snap, "finer") === null}
        onClick={() => scaleSnap("finer")}
      >
        {t("Finer")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label={t("Choose a coarser playlist snap")}
        disabled={nextPlaylistSnapScale(snap, "coarser") === null}
        onClick={() => scaleSnap("coarser")}
      >
        {t("Coarser")}
      </Button>
    </div>
  )
}

/**
 * Says what Play plays and changes it. The playhead only runs along the
 * playlist in song mode, so while the transport loops a pattern a button
 * offers to play the song instead.
 */
function SongControls() {
  const mode = useTransportStore((state) => state.mode)
  const playing = useTransportStore((state) => state.playing)
  const loop = useTransportStore((state) => state.loopSong)
  const follow = usePlaylistStore((state) => state.follow)
  // Audio and automation are the song's: a looping pattern plays neither.
  const silentInPattern = useProjectStore((state) =>
    state.project.playlist.clips.some((clip) => clip.content.type !== "pattern")
  )
  const playHint = useHint(
    t("Play the song: switches the transport from the pattern to the playlist and starts it")
  )
  const notice = mode === "pattern" && silentInPattern
  const modeHint = useHint(
    notice
      ? t("The transport loops one pattern, so the audio and automation clips on the playlist are not playing. Switch to Song to hear them")
      : mode === "song"
        ? t("Play plays the playlist from the song position")
        : t("Play loops the selected pattern. Switch to Song to play the playlist")
  )

  return (
    <div className="ml-auto flex min-w-0 items-center gap-1.5 pl-2">
      {mode === "pattern" && (
        <Button
          size="sm"
          className="shrink-0 gap-1 bg-brand text-brand-foreground hover:bg-brand/85"
          onClick={() => void runAction("playlist.playSong")}
          {...playHint}
        >
          <HugeiconsIcon icon={PlayIcon} strokeWidth={2} />
          {t("Play song")}
        </Button>
      )}
      <span
        role="status"
        data-slot="playlist-mode"
        data-notice={notice ? "" : undefined}
        className={cn(
          // In a narrow window the words give way before the buttons do.
          "min-w-0 truncate text-muted-foreground data-notice:text-warn",
          mode === "song" && playing && "text-brand"
        )}
        {...modeHint}
      >
        {mode === "song"
          ? t("Playing from the playlist")
          : notice
            ? t("Play loops the pattern: audio and automation play in Song mode")
            : t("Play loops the pattern")}
      </span>
      <ModeSwitch />
      <IconAction
        action="playlist.loopSong"
        icon={RepeatIcon}
        pressed={loop}
        about={t("at the end of the last clip, start again from the top")}
      />
      <IconAction
        action="playlist.follow"
        icon={Navigation03Icon}
        pressed={follow}
        about={t("scroll along while the song plays")}
      />
    </div>
  )
}

/** The strip above the timeline: clip list, tools, snap, zoom, song mode. */
export function PlaylistToolbar({ metrics }: { metrics: GridMetrics }) {
  const pickerOpen = usePlaylistStore((state) => state.pickerOpen)
  const tall = useViewportValue(metrics, () => metrics.tall)

  return (
    <ContextActions items={PANEL_MENU}>
      <div
        role="toolbar"
        aria-label={t("Playlist")}
        className="flex h-9 shrink-0 items-center gap-2 overflow-x-auto overflow-y-hidden border-b bg-chassis/40 px-1.5 whitespace-nowrap"
      >
        <IconAction
          action="playlist.patterns"
          icon={LeftToRightListBulletIcon}
          pressed={pickerOpen}
          about={t("show or hide the patterns, sounds and automations to place")}
        />
        <Tools />
        <StepToggle />
        <SnapMenu />
        <IconAction
          action="playlist.zoomToFit"
          icon={FitToScreenIcon}
          about={t("show the whole song")}
        />
        <IconAction
          action="playlist.tallTracks"
          icon={ArrowVerticalIcon}
          pressed={tall}
          about={t("make the tracks tall enough to draw automation curves in. Alt+wheel sets any height")}
        />
        <OverlapWarning metrics={metrics} />
        <SongControls />
      </div>
    </ContextActions>
  )
}
