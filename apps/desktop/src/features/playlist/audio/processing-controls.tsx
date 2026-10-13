import { useId, useState } from "react"
import type { ClipStretchQuality } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { backend } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { nextClipStretchQuality } from "./clip-quality-step"
import {
  beatFitRatio,
  fitRatio,
  processingCommand,
  type AudioClip,
} from "./processing"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export function ClipProcessingControls({ clips }: { clips: AudioClip[] }) {
  const [open, setOpen] = useState(false)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button size="sm" variant="outline" />}>
        {t("Stretch / tempo")}
      </DialogTrigger>
      <DialogContent className="max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("Stretch and pitch")}</DialogTitle>
          <DialogDescription>
            {t("Prepare selected playlist clips. Original audio stays intact; Apply is one undo step.")}
          </DialogDescription>
        </DialogHeader>
        {open && (
          <ProcessingForm clips={clips} onApplied={() => setOpen(false)} />
        )}
      </DialogContent>
    </Dialog>
  )
}
function ProcessingForm({
  clips,
  onApplied,
}: {
  clips: AudioClip[]
  onApplied: () => void
}) {
  const first = clips[0]
  const initial = first.content.stretch
  const [mode, setMode] = useState(initial?.mode ?? "tape")
  const [ratio, setRatio] = useState(
    String(initial?.mode === "spectral" ? initial.ratio : 1)
  )
  const [pitch, setPitch] = useState(String(first.content.pitch))
  const [quality, setQuality] = useState<ClipStretchQuality>(
    initial?.mode === "spectral" ? initial.quality : "standard"
  )
  const [formants, setFormants] = useState(
    initial?.mode === "spectral" && initial.formants
  )
  const [bpm, setBpm] = useState("")
  const [beats, setBeats] = useState("")
  const [candidates, setCandidates] = useState<
    { bpm: number; confidence: number }[] | null
  >(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const tempo = useProjectStore((s) => s.project.settings.tempoBpm)
  const id = useId()
  const singleSource = clips.every(
    (c) => c.content.sample === first.content.sample
  )
  async function run(work: () => Promise<void>) {
    setBusy(true)
    setError("")
    try {
      await work()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }
  async function apply() {
    await run(async () => {
      const stretch =
        mode === "spectral"
          ? {
              mode: "spectral" as const,
              ratio: Number(ratio),
              quality,
              formants,
            }
          : { mode: "tape" as const }
      const current = useProjectStore.getState().project.playlist.clips
      const latest = clips
        .map((clip) => current.find((c) => c.id === clip.id))
        .filter((clip): clip is AudioClip => clip?.content.type === "audio")
      if (latest.length !== clips.length)
        throw new Error(
          t("The selected clips changed. Close this window and select them again.")
        )
      const result = await backend.prepareClipCommand(
        processingCommand(latest, stretch, Number(pitch))
      )
      receivePatch(result.patch)
      onApplied()
    })
  }
  async function fit(useBeats: boolean) {
    await run(async () => {
      const fitted = useBeats
        ? beatFitRatio(
            Number(beats),
            (await backend.sampleInfoById(first.content.sample)).durationSecs,
            tempo
          )
        : fitRatio(Number(bpm), tempo)
      setMode("spectral")
      setRatio(String(fitted))
    })
  }
  return (
    <FieldGroup>
      <Field>
        <FieldLabel>{t("Playback mode")}</FieldLabel>
        <ToggleGroup
          disabled={busy}
          value={[mode]}
          onValueChange={(v) => {
            if (v[0]) setMode(v[0] as "tape" | "spectral")
          }}
        >
          <ToggleGroupItem value="tape">{t("Tape")}</ToggleGroupItem>
          <ToggleGroupItem value="spectral">
            {t("Independent stretch")}
          </ToggleGroupItem>
        </ToggleGroup>
        <FieldDescription>
          {t("Tape links pitch to speed. Independent stretch keeps duration and pitch separate.")}
        </FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor={id + "pitch"}>{t("Pitch (semitones)")}</FieldLabel>
        <Input
          id={id + "pitch"}
          type="number"
          min={mode === "spectral" ? -24 : -48}
          max={mode === "spectral" ? 24 : 48}
          step="0.01"
          value={pitch}
          onChange={(e) => setPitch(e.target.value)}
          disabled={busy}
        />
      </Field>
      {mode === "spectral" && (
        <>
          <Field>
            <FieldLabel htmlFor={id + "ratio"}>{t("Duration multiplier")}</FieldLabel>
            <Input
              id={id + "ratio"}
              type="number"
              min="0.25"
              max="4"
              step="0.01"
              value={ratio}
              onChange={(e) => setRatio(e.target.value)}
              disabled={busy}
            />
            <FieldDescription>
              {t("1 keeps the source duration; 2 doubles it. Trims and fades follow the same source interval.")}
            </FieldDescription>
          </Field>
          <Field>
            <FieldLabel>{t("Quality")}</FieldLabel>
            <ToggleGroup
              disabled={busy}
              value={[quality]}
              onValueChange={(v) => {
                if (v[0]) setQuality(v[0] as ClipStretchQuality)
              }}
            >
              {(["fast", "standard", "high"] as const).map((q) => (
                <ToggleGroupItem key={q} value={q}>
                  {q === "fast" ? t("Fast") : q === "high" ? t("High") : t("Standard")}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                aria-label={t("Choose the previous clip stretch quality")}
                disabled={
                  busy || nextClipStretchQuality(quality, "previous") === null
                }
                onClick={() => {
                  const next = nextClipStretchQuality(quality, "previous")
                  if (next !== null) setQuality(next)
                }}
              >
                {t("Previous")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                aria-label={t("Choose the next clip stretch quality")}
                disabled={busy || nextClipStretchQuality(quality, "next") === null}
                onClick={() => {
                  const next = nextClipStretchQuality(quality, "next")
                  if (next !== null) setQuality(next)
                }}
              >
                {t("Next")}
              </Button>
            </div>
          </Field>
          <Field orientation="horizontal">
            <Checkbox
              id={id + "formants"}
              checked={formants}
              onCheckedChange={setFormants}
              disabled={busy}
            />
            <FieldLabel htmlFor={id + "formants"}>{t("Preserve formants")}</FieldLabel>
          </Field>
        </>
      )}
      <Field>
        <FieldLabel htmlFor={id + "bpm"}>{t("Source BPM")}</FieldLabel>
        <Input
          id={id + "bpm"}
          type="number"
          min="1"
          value={bpm}
          onChange={(e) => setBpm(e.target.value)}
          disabled={busy}
        />
        <div className="flex gap-2">
          <Button
            variant="outline"
            disabled={busy || !singleSource}
            onClick={() =>
              void run(async () =>
                setCandidates(
                  await backend.detectClipTempo(first.content.sample)
                )
              )
            }
          >
            {t("Detect tempo")}
          </Button>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => void fit(false)}
          >
            {t("Fit to")}{" "}{tempo} BPM
          </Button>
        </div>
        <FieldDescription>
          {t("Fitting sets a fixed multiplier. Later project tempo changes do not automatically refit. Detection uses the first whole source file and can confuse half or double tempo.")}
        </FieldDescription>
      </Field>
      {candidates !== null && (
        <Field>
          <FieldDescription>
            {candidates.length
              ? t("Choose a candidate, then Fit. Scores show relative periodicity, not certainty.")
              : t("No reliable tempo found. Enter source BPM or the whole file's beat count.")}
          </FieldDescription>
          <div className="flex flex-wrap gap-2">
            {candidates.map((c) => (
              <Button
                key={c.bpm}
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => setBpm(String(c.bpm))}
              >
                {c.bpm.toFixed(1)} BPM ({Math.round(c.confidence * 100)}%)
              </Button>
            ))}
          </div>
        </Field>
      )}
      <Field>
        <FieldLabel htmlFor={id + "beats"}>
          {t("Beats in the whole source file")}
        </FieldLabel>
        <Input
          id={id + "beats"}
          type="number"
          min="0.01"
          value={beats}
          onChange={(e) => setBeats(e.target.value)}
          disabled={busy}
        />
        <Button
          variant="outline"
          disabled={busy || !singleSource}
          onClick={() => void fit(true)}
        >
          {t("Fit beat length to")}{" "}{tempo} BPM
        </Button>
        <FieldDescription>
          {t("Beat-count fitting requires the selected clips to use one source file.")}
        </FieldDescription>
      </Field>
      {error && <FieldError role="alert">{error}</FieldError>}
      <Button disabled={busy} onClick={() => void apply()}>
        {busy && <Spinner />}
        {busy
          ? t("Preparing audio…")
          : message("Apply to {count} clip{count2}", { count: plainNumber(clips.length), count2: clips.length === 1 ? "" : t("s") })}
      </Button>
    </FieldGroup>
  )
}
