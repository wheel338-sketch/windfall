import { PlayIcon, StopIcon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useEffect, useRef, useState, type ReactNode } from "react"

import type { LoopEntry } from "@/bindings"
import { Waveform, type WaveformHandle } from "@/components/audio"
import { Button } from "@/components/ui/button"
import { insertFactoryLoop } from "@/features/loops/insert"
import { songTick } from "@/features/playlist/ops"
import { reportError } from "@/lib/errors"
import { openProjectPath } from "@/lib/flows/project"
import { backend } from "@/lib/ipc"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"
import { useChannel } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"

import { addToPlaylist, addToRack, replaceChannelSample } from "./commands"
import { formatChannels, formatDuration, formatSampleRate } from "./format"
import { previewEnded, requestPreview, stopPreview } from "./preview"
import { PREVIEW_WAVE } from "./preview-colors"
import { useBrowserStore, type Selection } from "./store"
import { extensionStart, isUnder, parseRowId } from "./tree-model"
import { translatedText as t, message as i18nMessage } from "@/lib/i18n"

function PaneName({ selection }: { selection: Selection }) {
  const cut = extensionStart(selection.name, selection.kind)
  return (
    <p className="min-w-0 flex-1 truncate font-medium" title={selection.path}>
      {selection.name.slice(0, cut)}
      <span className="font-normal text-muted-foreground/70">
        {selection.name.slice(cut)}
      </span>
    </p>
  )
}

function channelColor(color: number): string {
  return `#${color.toString(16).padStart(6, "0")}`
}

/** Sets the selected channel's sample. Says which channel that is. */
function ReplaceButton({ path }: { path: string }) {
  const channel = useChannel(useUiStore((state) => state.selectedChannel))
  const instrument = channel?.source.type === "instrument"
  const explanation = !channel
    ? t("Select a channel in the rack to replace its sample")
    : instrument
      ? i18nMessage("{channelName} plays an instrument, so it has no sample to replace. Add this sound as a new channel instead", { channelName: channel.name })
      : i18nMessage("Make the channel {channelName} play this sound instead", { channelName: channel.name })
  const hint = useHint(explanation)

  return (
    // A disabled button takes no pointer events, so the wrapper explains it.
    <span
      className="flex min-w-0 @[232px]/browser:col-span-2"
      title={explanation}
      {...hint}
    >
      <Button
        variant="outline"
        size="sm"
        disabled={!channel || instrument}
        aria-label={
          channel
            ? i18nMessage("Replace the sample of {channelName}", { channelName: channel.name })
            : t("Replace selected channel's sample")
        }
        className="min-w-0 flex-1 justify-start gap-1.5"
        onClick={() => void replaceChannelSample(path)}
      >
        {channel ? (
          <>
            <span
              aria-hidden
              className="size-2 shrink-0 rounded-[2px]"
              style={{ backgroundColor: channelColor(channel.color) }}
            />
            <span className="truncate">{t("Replace")}{" "}{channel.name}</span>
          </>
        ) : (
          <span className="truncate">{t("Replace sample")}</span>
        )}
      </Button>
    </span>
  )
}

/** Puts the sound on the song's timeline as an audio clip. */
function PlaylistButton({ path }: { path: string }) {
  const hint = useHint(
    t("Put this sound on the playlist as an audio clip, at the song position on a new track. Or drag it onto the timeline")
  )
  return (
    <Button
      variant="outline"
      size="sm"
      className="min-w-0"
      onClick={() => void addToPlaylist(path)}
      {...hint}
    >
      <span className="truncate">{t("Add to playlist")}</span>
    </Button>
  )
}

/**
 * The factory loop a selected sound is, or null. The factory library keeps
 * each starter loop at Loops/<genre>/<name>.wav, which is how the Loops tab's
 * catalog is matched to a file here.
 */
function useFactoryLoop(selection: Selection): LoopEntry | null {
  const generation = useProjectGeneration()
  const factory = useBrowserStore((state) =>
    state.roots.find((root) => root.kind === "factory")
  )
  const rootPath = parseRowId(selection.id)?.root ?? null
  const relative =
    factory && rootPath === factory.path && isUnder(selection.path, factory.path)
      ? selection.path
          .slice(factory.path.length)
          .replaceAll("\\", "/")
          .replace(/^\//, "")
      : null
  const [loop, setLoop] = useState<LoopEntry | null>(null)
  useEffect(() => {
    if (relative === null || !relative.startsWith("Loops/")) return undefined
    let live = true
    void backend.loopCatalog().then(
      (catalog) => {
        if (!live) return
        const found = catalog.entries.find(
          (entry) => `Loops/${entry.genre}/${entry.name}.wav` === relative
        )
        setLoop(found ?? null)
      },
      // Without the catalog the sound is still an ordinary file.
      () => undefined
    )
    return () => {
      live = false
    }
  }, [relative, generation])
  return relative === null ? null : loop
}

/**
 * Puts a factory loop on the playlist the way the Loops tab does: made again
 * at the song's tempo, at the song position on a new track. Dragging the file
 * still brings the recording as it is, at the tempo it was made at.
 */
function FitLoopButton({ loop }: { loop: LoopEntry }) {
  const tempo = useProjectStore((state) => state.project.settings.tempoBpm)
  const hint = useHint(
    i18nMessage("This factory loop was made at {source} BPM. It is added at the song's {tempo} BPM, as in the Loops tab. Drag it onto the timeline to keep {source} BPM", { source: loop.tempo, tempo })
  )
  async function insert() {
    try {
      // The catalog names the project state the loop is fitted for.
      const catalog = await backend.loopCatalog()
      await insertFactoryLoop(
        {
          id: loop.id,
          name: loop.name,
          bars: loop.bars,
          key: null,
          generation: catalog.generation,
          revision: catalog.revision,
        },
        songTick()
      )
    } catch (error) {
      reportError(error, t("Could not insert the factory loop"))
    }
  }
  return (
    <Button
      variant="outline"
      size="sm"
      className="min-w-0"
      onClick={() => void insert()}
      {...hint}
    >
      <span className="truncate">
        {i18nMessage("Add at {tempo} BPM", { tempo })}
      </span>
    </Button>
  )
}

function Facts({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-4 items-center gap-2.5 overflow-hidden font-readout text-[10.5px] whitespace-nowrap text-muted-foreground">
      {children}
    </div>
  )
}

/**
 * A backend message about the selected file, with the file's whole path cut
 * down to its name. The pane is narrow, and the name is already above it.
 */
function withoutFolder(message: string, selection: Selection): string {
  const { path, name } = selection
  const otherSlashes = path.includes("\\")
    ? path.replaceAll("\\", "/")
    : path.replaceAll("/", "\\")
  return message
    .replaceAll("\\\\?\\", "")
    .replaceAll(path, name)
    .replaceAll(otherSlashes, name)
}

function SoundPane({ selection }: { selection: Selection }) {
  const { path } = selection
  const info = useBrowserStore((state) =>
    state.info?.path === path ? state.info : null
  )
  const startedAt = useBrowserStore((state) =>
    state.playing?.path === path ? state.playing.startedAt : null
  )
  const previewError = useBrowserStore((state) =>
    state.previewError?.path === path ? state.previewError.message : null
  )
  const readError =
    info?.status === "error" ? withoutFolder(info.message, selection) : null
  // A file that cannot be read cannot be played either, for the same
  // reason. It is said once, where the waveform would be.
  const playError =
    readError === null && previewError !== null
      ? withoutFolder(previewError, selection)
      : null
  const waveform = useRef<WaveformHandle>(null)
  const duration = info?.status === "ready" ? info.info.durationSecs : null

  // The engine does not report where the preview is, so the playhead runs
  // on the clock from the moment the engine took the sound. It is moved
  // without rendering, like every other realtime value.
  useEffect(() => {
    const handle = waveform.current
    if (startedAt === null || duration === null || !handle) return undefined
    let frame = 0
    const tick = () => {
      const position =
        (performance.now() - startedAt) / 1000 / Math.max(duration, 0.001)
      if (position >= 1) {
        handle.setPlayhead(null)
        previewEnded(path)
        return
      }
      handle.setPlayhead(position)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => {
      cancelAnimationFrame(frame)
      handle.setPlayhead(null)
    }
  }, [startedAt, duration, path])

  const playing = startedAt !== null
  const loop = useFactoryLoop(selection)

  return (
    <>
      <div className="flex h-6 items-center gap-1">
        <PaneName selection={selection} />
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={playing ? t("Stop preview") : t("Play preview")}
          className={playing ? "text-brand" : "text-muted-foreground"}
          onClick={() => (playing ? stopPreview() : requestPreview(path))}
        >
          <HugeiconsIcon
            icon={playing ? StopIcon : PlayIcon}
            strokeWidth={2}
            className="fill-current"
          />
        </Button>
      </div>

      {info?.status === "ready" ? (
        <Waveform
          ref={waveform}
          role="img"
          aria-label={i18nMessage("Waveform of {selectionName}", { selectionName: selection.name })}
          peaks={info.info.peaks}
          color={PREVIEW_WAVE}
          className="h-12 shrink-0 cursor-pointer bg-display"
          onClick={() => requestPreview(path)}
        />
      ) : readError !== null ? (
        <p
          role="alert"
          title={readError}
          className="flex h-12 shrink-0 items-center overflow-hidden rounded-sm border border-dashed border-destructive/50 px-2 text-destructive"
        >
          <span className="line-clamp-2">
            {t("Could not read this sound.")}{" "}{readError}
          </span>
        </p>
      ) : (
        <div
          role="status"
          aria-label={t("Reading the sound")}
          className="h-12 shrink-0 animate-pulse rounded-sm bg-display/70"
        />
      )}

      {playError !== null ? (
        <p
          role="alert"
          title={playError}
          className="h-4 truncate text-destructive"
        >
          {t("Could not play it.")}{" "}{playError}
        </p>
      ) : info?.status === "ready" ? (
        <Facts>
          <span>{formatDuration(info.info.durationSecs)}</span>
          <span>{formatSampleRate(info.info.sampleRate)}</span>
          <span>{formatChannels(info.info.channels)}</span>
          {loop && <span>{i18nMessage("{tempo} BPM loop", { tempo: loop.tempo })}</span>}
        </Facts>
      ) : (
        <Facts>{null}</Facts>
      )}

      <div className="grid grid-cols-1 gap-1 @[232px]/browser:grid-cols-2">
        <Button
          size="sm"
          className="min-w-0"
          onClick={() => void addToRack(path)}
        >
          <span className="truncate">{t("Add to rack")}</span>
        </Button>
        {loop ? <FitLoopButton loop={loop} /> : <PlaylistButton path={path} />}
        <ReplaceButton path={path} />
      </div>
    </>
  )
}

function ProjectPane({ selection }: { selection: Selection }) {
  return (
    <>
      <div className="flex h-6 items-center">
        <PaneName selection={selection} />
      </div>
      <p className="flex-1 text-muted-foreground">
        {t("A Windfall project. Opening it replaces the project you have open.")}
      </p>
      <Button
        size="sm"
        variant="outline"
        className="self-start"
        onClick={() => void openProjectPath(selection.path)}
      >
        {t("Open project")}
      </Button>
    </>
  )
}

function PaneHint() {
  return (
    <p className="m-auto max-w-56 text-center text-balance text-muted-foreground">
      {t("Select a sound to hear it. Double-click it to add it to the channel rack, or drag it there or onto the playlist.")}
    </p>
  )
}

/**
 * The strip under the tree for whatever is selected: a sound's waveform and
 * facts with the two ways to use it, or a project file. Its height does not
 * depend on the selection, so the tree does not jump while arrowing.
 */
export function PreviewPane() {
  const selection = useBrowserStore((state) => state.selected)

  return (
    <section
      aria-label={t("Preview")}
      className="flex h-[192px] shrink-0 flex-col gap-1 border-t bg-chassis/60 p-1.5 @[232px]/browser:h-[164px]"
    >
      {selection?.kind === "audio" ? (
        <SoundPane key={selection.path} selection={selection} />
      ) : selection?.kind === "project" ? (
        <ProjectPane selection={selection} />
      ) : (
        <PaneHint />
      )}
    </section>
  )
}
