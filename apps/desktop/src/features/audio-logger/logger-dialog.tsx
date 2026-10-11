import { useEffect } from "react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useProjectStore } from "@/lib/store/project"
import {
  closeAudioLogger,
  configureAudioLogger,
  dumpAudioLogger,
  refreshAudioLogger,
  useAudioLoggerStore,
} from "./logger-store"
import { sourceKey, type AudioLoggerSourceStatus } from "./types"

/** How often the open dialog asks how much each history holds. */
const POLL_MS = 1000

/** History lengths offered, in seconds. */
const LENGTHS = [30, 60, 120, 300, 600]

/** Trailing lengths a dump can keep, in seconds. Zero keeps everything. */
const DUMP_LENGTHS = [10, 30, 60, 120, 0]

export function formatSeconds(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)} s`
  const minutes = Math.floor(seconds / 60)
  const rest = Math.round(seconds - minutes * 60)
  return rest === 0 ? `${minutes} min` : `${minutes} min ${rest} s`
}

export function formatMemory(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function Choice({
  label,
  value,
  items,
  onChange,
  disabled,
}: {
  label: string
  value: number
  items: { value: number; label: string }[]
  onChange: (value: number) => void
  disabled: boolean
}) {
  return (
    <Select
      items={items}
      value={value}
      disabled={disabled}
      onValueChange={(next: number | null) => {
        if (next !== null) onChange(next)
      }}
    >
      <SelectTrigger aria-label={label}>
        <SelectValue placeholder="Not available" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {items.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

function SourceRow({
  status,
  disabled,
}: {
  status: AudioLoggerSourceStatus
  disabled: boolean
}) {
  const empty = status.availableFrames === 0
  return (
    <li className="flex flex-col gap-1 rounded-md border p-2">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium">{status.label}</p>
          <p className="text-xs text-muted-foreground">
            {status.active ? "Logging" : "Not receiving audio"} ·{" "}
            {formatSeconds(status.availableSeconds)} of{" "}
            {formatSeconds(status.capacitySeconds)} at {status.sampleRate} Hz ·{" "}
            {formatMemory(status.memoryBytes)}
            {status.droppedFrames > 0 &&
              ` · ${status.droppedFrames} frames dropped`}
          </p>
        </div>
        <Button
          size="sm"
          variant="secondary"
          disabled={disabled || empty}
          aria-label={`Add logged ${status.label} to playlist`}
          onClick={() => void dumpAudioLogger(status.source)}
        >
          Add to playlist
        </Button>
      </div>
      {status.error && (
        <p role="alert" className="text-xs text-destructive">
          {status.error}
        </p>
      )}
    </li>
  )
}

/**
 * The audio logger: what it keeps in the background, how much each history
 * holds, and the way to put the end of one on the playlist.
 */
export function AudioLoggerDialog() {
  const s = useAudioLoggerStore()
  const mixerTracks = useProjectStore((state) => state.project.mixer.tracks)
  useEffect(() => {
    if (!s.open) return
    const timer = window.setInterval(() => void refreshAudioLogger(), POLL_MS)
    return () => window.clearInterval(timer)
  }, [s.open])

  const state = s.state
  const settings = state?.settings
  const disabled = s.busy || !state
  // The master is a source of its own, so it is not offered as a track.
  const loggable = mixerTracks.slice(1)
  const inputIndex = settings?.input
    ? s.inputs.findIndex(
        (input) =>
          input.host === settings.input?.host &&
          input.device === settings.input?.device
      )
    : -1
  const inputItems = [
    { value: -1, label: "No input" },
    ...s.inputs.map((input, index) => ({
      value: index,
      label: `${input.device} (${input.host})`,
    })),
  ]
  // A saved input the machine no longer lists stays visible, so it can be
  // seen and changed instead of silently becoming "No input".
  if (settings?.input && inputIndex < 0)
    inputItems.push({
      value: -2,
      label: `${settings.input.device} (unavailable)`,
    })
  const lengths = LENGTHS.filter(
    (seconds) =>
      !state || (seconds >= state.minSeconds && seconds <= state.maxSeconds)
  )
  if (settings && !lengths.includes(settings.seconds))
    lengths.push(settings.seconds)

  return (
    <Dialog
      open={s.open}
      onOpenChange={(open) => {
        if (!open && !s.busy) closeAudioLogger()
      }}
    >
      <DialogContent showCloseButton={!s.busy}>
        <DialogHeader>
          <DialogTitle>Audio logger</DialogTitle>
          <DialogDescription>
            Keeps the last minutes of audio in memory while you work, so
            something you played without recording can still be added to the
            playlist. Nothing is written to disk until you add it.
          </DialogDescription>
        </DialogHeader>
        {(s.error || state?.error) && (
          <Alert variant="destructive">
            <AlertDescription>{s.error || state?.error}</AlertDescription>
          </Alert>
        )}
        {settings && state && (
          <FieldGroup>
            <Field orientation="horizontal">
              <Checkbox
                id="logger-enabled"
                checked={settings.enabled}
                disabled={disabled}
                onCheckedChange={(checked) =>
                  void configureAudioLogger({ enabled: checked === true })
                }
              />
              <FieldLabel htmlFor="logger-enabled">
                Keep audio in the background
              </FieldLabel>
            </Field>
            <Field>
              <FieldLabel>History length</FieldLabel>
              <Choice
                label="History length"
                value={settings.seconds}
                disabled={disabled}
                items={lengths
                  .slice()
                  .sort((a, b) => a - b)
                  .map((seconds) => ({
                    value: seconds,
                    label: formatSeconds(seconds),
                  }))}
                onChange={(seconds) => void configureAudioLogger({ seconds })}
              />
              <FieldDescription>
                Changing the length or the sources starts the history over.
                Using {formatMemory(state.memoryBytes)} now.
              </FieldDescription>
            </Field>
            <Field orientation="horizontal">
              <Checkbox
                id="logger-master"
                checked={settings.master}
                disabled={disabled}
                onCheckedChange={(checked) =>
                  void configureAudioLogger({ master: checked === true })
                }
              />
              <FieldLabel htmlFor="logger-master">Master output</FieldLabel>
            </Field>
            <Field>
              <FieldLabel>Mixer tracks, after their effects</FieldLabel>
              <div className="flex max-h-28 flex-col gap-1 overflow-y-auto">
                {loggable.map((track) => {
                  const checked = settings.tracks.includes(track.id)
                  const full =
                    !checked && settings.tracks.length >= state.maxTracks
                  return (
                    <div key={track.id} className="flex items-center gap-2">
                      <Checkbox
                        id={`logger-track-${track.id}`}
                        checked={checked}
                        disabled={disabled || full}
                        onCheckedChange={(next) =>
                          void configureAudioLogger({
                            tracks:
                              next === true
                                ? [...settings.tracks, track.id]
                                : settings.tracks.filter(
                                    (id) => id !== track.id
                                  ),
                          })
                        }
                      />
                      <FieldLabel htmlFor={`logger-track-${track.id}`}>
                        {track.name}
                      </FieldLabel>
                    </div>
                  )
                })}
              </div>
              <FieldDescription>
                At most {state.maxTracks} tracks. A logged track is what the
                audio editor can pick up after the fact.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>Audio input</FieldLabel>
              <Choice
                label="Logged audio input"
                value={
                  settings.input ? (inputIndex >= 0 ? inputIndex : -2) : -1
                }
                disabled={disabled}
                items={inputItems}
                onChange={(index) => {
                  if (index === -2) return
                  const input = s.inputs[index]
                  void configureAudioLogger({
                    input: input
                      ? {
                          host: input.host,
                          device: input.device,
                          left: 0,
                          right: input.channels > 1 ? 1 : null,
                        }
                      : null,
                  })
                }}
              />
              <FieldDescription>
                The input stays open in the background and gives way to a
                recording take on the same device.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>Add to playlist</FieldLabel>
              <Choice
                label="Length to add"
                value={s.dumpSeconds}
                disabled={disabled}
                items={DUMP_LENGTHS.map((seconds) => ({
                  value: seconds,
                  label:
                    seconds === 0
                      ? "Everything kept"
                      : `Last ${formatSeconds(seconds)}`,
                }))}
                onChange={(dumpSeconds) =>
                  useAudioLoggerStore.setState({ dumpSeconds })
                }
              />
              <FieldDescription>
                The clip starts at the playhead on a new playlist track. It is
                one undo step.
              </FieldDescription>
            </Field>
            {state.sources.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {settings.enabled
                  ? "No source is being logged."
                  : "The audio logger is off."}
              </p>
            ) : (
              <ul aria-label="Logged sources" className="flex flex-col gap-2">
                {state.sources.map((status) => (
                  <SourceRow
                    key={sourceKey(status.source)}
                    status={status}
                    disabled={s.busy}
                  />
                ))}
              </ul>
            )}
          </FieldGroup>
        )}
      </DialogContent>
    </Dialog>
  )
}
