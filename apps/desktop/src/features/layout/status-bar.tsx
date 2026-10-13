import { ImportedSoundsButton } from "@/features/flp-import/retained-dialog"
import { ContextActions } from "@/components/context-actions"
import { runAction } from "@/lib/actions"
import { useEngineStore } from "@/lib/store/engine"
import { useHint, useHintStore } from "@/lib/store/hint"
import { useDirty, useProjectPath } from "@/lib/store/selectors"
import { fileName, formatSampleRate } from "@/lib/time"
import { cn } from "@/lib/utils"

import { STATUS_MENU } from "./chrome-menus"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

function Dot({ className }: { className: string }) {
  return (
    <span
      aria-hidden
      className={cn("size-1.5 shrink-0 rounded-full", className)}
    />
  )
}

function EngineStatus() {
  const status = useEngineStore((state) => state.status)
  const hint = useHint(t("Audio output. Click to choose a device or buffer size"))

  // What the project's effects and instruments add on top of the buffer.
  const pluginMs =
    status && status.sampleRate > 0
      ? (status.latencyFrames / status.sampleRate) * 1000
      : 0

  let content = (
    <>
      <Dot className="bg-foreground/30" />
      <span>{t("Starting audio…")}</span>
    </>
  )
  if (status?.running) {
    content = (
      <>
        <Dot className="bg-ok" />
        <span className="truncate text-foreground/85">
          {status.host} {status.device}
        </span>
        <span className="shrink-0 font-readout">
          {formatSampleRate(status.sampleRate)}
        </span>
        <span className="shrink-0 font-readout">{status.bufferFrames}{" "}{t("smp")}</span>
        <span className="shrink-0 font-readout">
          {status.latencyMs.toFixed(1)} ms
        </span>
        {pluginMs > 0 && (
          <span
            className="shrink-0 font-readout"
            title={message("Effects and instruments delay the output by {latencyFrames} samples. Every track is lined up with the slowest.", { latencyFrames: plainNumber(status.latencyFrames) })}
          >
            +{pluginMs.toFixed(1)} ms
          </span>
        )}
      </>
    )
  } else if (status) {
    content = (
      <>
        <Dot className="bg-destructive" />
        <span className="truncate text-destructive">
          {t("No audio.")}{" "}{status.error ?? t("The output is not running.")}
        </span>
      </>
    )
  }

  return (
    <button
      type="button"
      aria-label={t("Audio output status. Open audio settings")}
      onClick={() => void runAction("options.settings")}
      // It gives way before the hint does: the name of the device is cut
      // short first.
      className="flex h-full max-w-[45%] min-w-0 shrink-[4] items-center gap-2 px-2 outline-none hover:bg-foreground/5 focus-visible:bg-foreground/10 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      {...hint}
    >
      {content}
    </button>
  )
}

function Hint() {
  const text = useHintStore((state) => state.text)
  const notice = useHintStore((state) => state.notice)
  const shown = notice ?? text
  return (
    <p
      // A notice is about what was just done, so it is read out. The hint
      // changes with every pointer move and is not.
      role={notice === null ? undefined : "status"}
      aria-live={notice === null ? "off" : "polite"}
      data-slot="status-hint"
      data-notice={notice === null ? undefined : ""}
      // Cut short only when the window leaves no room. The whole line is
      // then a hover away.
      title={shown ?? undefined}
      // As wide as what it says, and the last of the three to be cut.
      className="min-w-0 flex-[1_1_auto] truncate px-2 data-notice:text-warn"
    >
      {shown}
    </p>
  )
}

function SaveState() {
  const dirty = useDirty()
  const path = useProjectPath()

  let label = t("Saved")
  if (path === null) label = "Not saved to a file yet"
  else if (dirty) label = "Edited"

  return (
    <div
      className="ml-auto flex min-w-0 shrink-[4] items-center gap-2 px-2"
      title={path ?? undefined}
    >
      <Dot className={dirty ? "bg-brand" : "bg-foreground/30"} />
      <span className="shrink-0">{label}</span>
      {path !== null && (
        <span className="truncate text-foreground/85">{fileName(path)}</span>
      )}
    </div>
  )
}

/**
 * The bottom strip: the audio engine on the left, help for the control
 * under the pointer in the middle, and the state of the file on the right.
 */
export function StatusBar() {
  return (
    <ContextActions items={STATUS_MENU}>
      <footer className="flex h-6 shrink-0 items-center border-t bg-chassis text-[0.6875rem] text-muted-foreground">
        <EngineStatus />
        <Hint />
        <ImportedSoundsButton />
        <SaveState />
      </footer>
    </ContextActions>
  )
}
