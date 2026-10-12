import { useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import type { MidiExpressionMode, MidiImportOptions, MidiImportPreview } from "@/bindings"
import { PROJECT_TIMEBASES } from "@/features/project-info/settings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Choice } from "@/features/export/choice"
import { projectName } from "@/lib/flows/project"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { selectedPatternId } from "@/lib/store/selectors"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { fileName } from "@/lib/time"

import { DEFAULT_IMPORT, midiExportOptions } from "./options"

function Check({
  id,
  label,
  checked,
  disabled,
  onChange,
}: {
  id: string
  label: string
  checked: boolean
  disabled: boolean
  onChange(value: boolean): void
}) {
  return (
    <Field orientation="horizontal" data-disabled={disabled || undefined}>
      <Checkbox
        id={id}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(value) => onChange(value === true)}
      />
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
    </Field>
  )
}

type FormProps = { done(): void; busy(value: boolean): void }

function counted(count: number, noun: string) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`
}

function ImportForm({ done, busy }: FormProps) {
  const [path, setPath] = useState("")
  const [options, setOptions] = useState(DEFAULT_IMPORT)
  const [preview, setPreview] = useState<MidiImportPreview | null>(null)
  const [reading, setReading] = useState(false)
  const [importing, setImporting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const request = useRef(0)
  const token = useRef<number | null>(null)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      request.current += 1
      if (token.current !== null)
        void backend.midiDiscard(token.current).catch(() => {})
    }
  }, [])

  async function review(file: string, draft: MidiImportOptions) {
    const current = ++request.current
    if (token.current !== null)
      void backend.midiDiscard(token.current).catch(() => {})
    token.current = null
    setPreview(null)
    setError(null)
    setReading(true)
    try {
      const result = await backend.midiPreview(file, draft)
      if (!mounted.current || current !== request.current) {
        void backend.midiDiscard(result.token).catch(() => {})
        return
      }
      token.current = result.token
      setPreview(result)
    } catch (failure) {
      if (mounted.current && current === request.current)
        setError(errorMessage(failure))
    } finally {
      if (mounted.current && current === request.current) setReading(false)
    }
  }

  function change<K extends keyof MidiImportOptions>(
    key: K,
    value: MidiImportOptions[K]
  ) {
    const draft = { ...options, [key]: value }
    setOptions(draft)
    if (path) void review(path, draft)
  }

  async function choose() {
    try {
      const file = await backend.pickMidiFile()
      if (file === null || !mounted.current) return
      setPath(file)
      await review(file, options)
    } catch (failure) {
      if (mounted.current) setError(errorMessage(failure))
    }
  }

  async function append() {
    if (!preview || importing || reading) return
    setImporting(true)
    busy(true)
    setError(null)
    try {
      const result = await backend.importMidi(preview.token)
      receivePatch(result.patch)
      token.current = null
      toast.success("Imported MIDI", {
        description: `${counted(preview.notes, "note")} added. Undo removes the entire import.`,
      })
      useUiStore.getState().showCenterTab("playlist")
      done()
    } catch (failure) {
      setError(errorMessage(failure))
    } finally {
      busy(false)
      if (mounted.current) setImporting(false)
    }
  }

  return (
    <FieldGroup>
      <Field>
        <FieldLabel>MIDI file</FieldLabel>
        <Button
          variant="outline"
          disabled={importing}
          onClick={() => void choose()}
        >
          Choose MIDI file…
        </Button>
        <FieldDescription>
          {path ? fileName(path) : "Choose a .mid or .midi file."}
        </FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-bars">Patterns</FieldLabel>
        <Choice
          id="midi-bars"
          value={options.bars}
          disabled={importing}
          options={[
            { value: 0, label: "One pattern per part" },
            { value: 1, label: "Split every bar" },
            { value: 4, label: "Split every 4 bars" },
            { value: 8, label: "Split every 8 bars" },
          ]}
          onChange={(value) => change("bars", value)}
        />
        <FieldDescription>
          Long parts split at the pattern limit. Notes across a split retrigger.
        </FieldDescription>
      </Field>
      <FieldSet>
        <FieldLegend>Import settings</FieldLegend>
        <Check
          id="midi-share"
          label="Share identical sections"
          checked={options.sharePatterns}
          disabled={importing || options.bars === 0}
          onChange={(value) => change("sharePatterns", value)}
        />
        <Check
          id="midi-drums"
          label="Use factory drum samples"
          checked={options.factoryDrums}
          disabled={importing}
          onChange={(value) => change("factoryDrums", value)}
        />
        <Check
          id="midi-tempo"
          label="Import tempo and tempo changes"
          checked={options.tempo}
          disabled={importing}
          onChange={(value) => change("tempo", value)}
        />
        <Check
          id="midi-meter"
          label="Import the first time signature"
          checked={options.timeSignature}
          disabled={importing}
          onChange={(value) => change("timeSignature", value)}
        />
        <Check
          id="midi-mix"
          label="Import channel volume and pan"
          checked={options.channelMix}
          disabled={importing}
          onChange={(value) => change("channelMix", value)}
        />
        <Check id="midi-note-expression" label="Import pitch bend and controllers as note expression" checked={options.noteExpression ?? true} disabled={importing} onChange={(value) => change("noteExpression", value)} />
      </FieldSet>
      <div role="status" aria-live="polite">
        {reading && "Reading MIDI…"}
        {preview && (
          <>
            <p>
              {counted(preview.channels.length, "channel")},{" "}
              {counted(preview.patterns, "pattern")},{" "}
              {counted(preview.clips, "clip")} and{" "}
              {counted(preview.notes, "note")} will be added.
            </p>
            {preview.channels.length > 0 && (
              <p>{preview.channels.join(", ")}</p>
            )}
            {preview.adjustments.length > 0 && (
              <ul className="list-disc pl-4">
                {preview.adjustments.map((line, index) => (
                  <li key={index}>{line}</li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>
      {error && <FieldError role="alert">{error}</FieldError>}
      <DialogFooter>
        <Button variant="outline" disabled={importing} onClick={done}>
          Cancel
        </Button>
        <Button
          disabled={!preview || reading || importing}
          onClick={() => void append()}
        >
          {importing ? "Importing…" : "Import"}
        </Button>
      </DialogFooter>
    </FieldGroup>
  )
}

function ExportForm({ done, busy }: FormProps) {
  const [options, setOptions] = useState(() => {
    const transport = useTransportStore.getState()
    const project = useProjectStore.getState().project
    return midiExportOptions(
      transport.mode,
      selectedPatternId(project, transport.pattern) ?? 0
    )
  })
  const [resolutionText, setResolutionText] = useState(() => String(options.ppq))
  const resolution = Number(resolutionText)
  const resolutionValid = resolutionText.trim() !== "" && Number.isInteger(resolution) && resolution >= 1 && resolution <= 32767
  const [working, setWorking] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function save() {
    if (!expressionValid || !resolutionValid) return
    setWorking(true)
    busy(true)
    setError(null)
    try {
      const path = await backend.pickMidiExportPath(projectName())
      if (path === null) return
      const saved = await backend.exportMidi(path, options)
      toast.success("Exported MIDI", { description: fileName(saved) })
      done()
    } catch (failure) {
      setError(errorMessage(failure))
    } finally {
      setWorking(false)
      busy(false)
    }
  }
  const expression = options.expression ?? { mode: "off", bendRange: 0, step: 0 }
  const expressionValid = Number.isInteger(expression.bendRange) && expression.bendRange >= 0 && expression.bendRange <= 96
    && Number.isInteger(expression.step) && expression.step >= 0 && expression.step <= 245760
  return (
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor="midi-mode">Export</FieldLabel>
        <Choice
          id="midi-mode"
          value={options.mode}
          disabled={working}
          options={[
            { value: "pattern", label: "The selected pattern" },
            { value: "song", label: "The whole song" },
          ]}
          onChange={(mode) => setOptions({ ...options, mode })}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-format">MIDI format</FieldLabel>
        <Choice
          id="midi-format"
          value={options.singleTrack ? "single" : "multi"}
          disabled={working}
          options={[
            { value: "multi", label: "Format 1 — separate tracks" },
            { value: "single", label: "Format 0 — one track" },
          ]}
          onChange={(value) =>
            setOptions({ ...options, singleTrack: value === "single" })
          }
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-ppq">Resolution</FieldLabel>
        <Choice
          id="midi-ppq"
          value={options.ppq}
          disabled={working}
          options={PROJECT_TIMEBASES.map((value) => ({ value, label: `${value} PPQ` }))}
          onChange={(ppq) => { setOptions({ ...options, ppq }); setResolutionText(String(ppq)) }}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-custom-ppq">Custom MIDI resolution (1–32767)</FieldLabel>
        <Input id="midi-custom-ppq" type="number" min={1} max={32767} value={resolutionText} aria-invalid={!resolutionValid} disabled={working} onChange={(event) => { const text = event.target.value; setResolutionText(text); const ppq = Number(text); if (text.trim() && Number.isInteger(ppq) && ppq >= 1 && ppq <= 32767) setOptions({ ...options, ppq }) }} />
        {!resolutionValid && <FieldError>Use a whole resolution from 1 to 32767 PPQ.</FieldError>}
      </Field>
      <FieldSet>
        <FieldLegend>Playback</FieldLegend>
        <Check
          id="midi-swing"
          label="Apply project swing"
          checked={options.swing}
          disabled={working}
          onChange={(swing) => setOptions({ ...options, swing })}
        />
        <Check
          id="midi-running"
          label="Use running status (smaller file)"
          checked={options.runningStatus}
          disabled={working}
          onChange={(runningStatus) =>
            setOptions({ ...options, runningStatus })
          }
        />
      </FieldSet>
      <FieldSet><FieldLegend>Note expression</FieldLegend>
        <Field><FieldLabel htmlFor="midi-expression-mode">Expression routing</FieldLabel>
          <Choice<MidiExpressionMode> id="midi-expression-mode" value={expression.mode} disabled={working}
            options={[{ value: "off", label: "Notes only" }, { value: "channelVoice", label: "Color-group channels" }, { value: "mpe", label: "MPE · one channel per note" }]}
            onChange={(mode) => setOptions({ ...options, expression: { ...expression, mode } })} /></Field>
        <Field><FieldLabel htmlFor="midi-expression-bend">Pitch-bend range (semitones; 0 = automatic)</FieldLabel>
          <Input id="midi-expression-bend" type="number" min={0} max={96} value={Number.isFinite(expression.bendRange) ? expression.bendRange : ""} disabled={working || expression.mode === "off"}
            onChange={(event) => setOptions({ ...options, expression: { ...expression, bendRange: event.target.valueAsNumber } })} /></Field>
        <Field><FieldLabel htmlFor="midi-expression-step">Expression spacing (ticks; 0 = automatic)</FieldLabel>
          <Input id="midi-expression-step" type="number" min={0} max={245760} value={Number.isFinite(expression.step) ? expression.step : ""} disabled={working || expression.mode === "off"}
            onChange={(event) => setOptions({ ...options, expression: { ...expression, step: event.target.valueAsNumber } })} /></Field>
        <FieldDescription>Color-group channels share bends and controllers. MPE assigns member channels and overrides note colors. Pitch bend, modulation, pan and saved curves are sampled; match the receiving instrument’s bend range.</FieldDescription>
      </FieldSet>
      <FieldDescription>
        Exports notes, tempo and meter. Audio clips, instrument sounds and
        effects are omitted. Tempo glides are sampled; more than 15 melodic
        tracks share MIDI channels.
      </FieldDescription>
      {error && <FieldError role="alert">{error}</FieldError>}
      <DialogFooter>
        <Button variant="outline" disabled={working} onClick={done}>
          Cancel
        </Button>
        <Button disabled={working || !expressionValid || !resolutionValid} onClick={() => void save()}>
          {working ? "Exporting…" : "Choose file and export…"}
        </Button>
      </DialogFooter>
    </FieldGroup>
  )
}

export function MidiDialogs() {
  const dialog = useUiStore((state) => state.dialog)
  const close = useUiStore((state) => state.closeDialog)
  const [busy, setBusy] = useState(false)
  const importing = dialog === "midiImport"
  const open = importing || dialog === "midiExport"
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) close()
      }}
    >
      <DialogContent
        showCloseButton={!busy}
        className="max-h-[calc(100dvh-4rem)] overflow-y-auto sm:max-w-lg"
      >
        <DialogHeader>
          <DialogTitle>{importing ? "Import MIDI" : "Export MIDI"}</DialogTitle>
          <DialogDescription>
            {importing
              ? "Append notes and arrangement to this project as one undo step. MIDI instruments use the subtractive synth unless drums are mapped."
              : "Save notes and tempo as a MIDI file. This exports no audio."}
          </DialogDescription>
        </DialogHeader>
        {open &&
          (importing ? (
            <ImportForm done={close} busy={setBusy} />
          ) : (
            <ExportForm done={close} busy={setBusy} />
          ))}
      </DialogContent>
    </Dialog>
  )
}
