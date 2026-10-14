import { localizeBackendText } from "@/lib/i18n"
import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
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
import { backend, errorSource } from "@/lib/ipc"
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
  useLanguageStore((state) => state.language)
  return (
    <Field orientation="horizontal" data-disabled={disabled || undefined}>
      <Checkbox
        id={id}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(value) => onChange(value === true)}
      />
      <FieldLabel htmlFor={id}>{t(label)}</FieldLabel>
    </Field>
  )
}

type FormProps = { done(): void; busy(value: boolean): void }

function ImportForm({ done, busy }: FormProps) {
  useLanguageStore((state) => state.language)
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
        setError(errorSource(failure))
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
      if (mounted.current) setError(errorSource(failure))
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
      toast.success(t("Imported MIDI"), {
        description: message("{count} notes added. Undo removes the entire import.", { count: preview.notes }),
      })
      useUiStore.getState().showCenterTab("playlist")
      done()
    } catch (failure) {
      setError(errorSource(failure))
    } finally {
      busy(false)
      if (mounted.current) setImporting(false)
    }
  }

  return (
    <FieldGroup>
      <Field>
        <FieldLabel>{t("MIDI file")}</FieldLabel>
        <Button
          variant="outline"
          disabled={importing}
          onClick={() => void choose()}
        >{t("Choose MIDI file…")}</Button>
        <FieldDescription>
          {path ? fileName(path) : t("Choose a .mid or .midi file.")}
        </FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-bars">{t("Patterns")}</FieldLabel>
        <Choice
          id="midi-bars"
          value={options.bars}
          disabled={importing}
          options={[
            { value: 0, label: t("One pattern per part") },
            { value: 1, label: t("Split every bar") },
            { value: 4, label: t("Split every 4 bars") },
            { value: 8, label: t("Split every 8 bars") },
          ]}
          onChange={(value) => change("bars", value)}
        />
        <FieldDescription>{t("Long parts split at the pattern limit. Notes across a split retrigger.")}</FieldDescription>
      </Field>
      <FieldSet>
        <FieldLegend>{t("Import settings")}</FieldLegend>
        <Check
          id="midi-share"
          label={t("Share identical sections")}
          checked={options.sharePatterns}
          disabled={importing || options.bars === 0}
          onChange={(value) => change("sharePatterns", value)}
        />
        <Check
          id="midi-drums"
          label={t("Use factory drum samples")}
          checked={options.factoryDrums}
          disabled={importing}
          onChange={(value) => change("factoryDrums", value)}
        />
        <Check
          id="midi-tempo"
          label={t("Import tempo and tempo changes")}
          checked={options.tempo}
          disabled={importing}
          onChange={(value) => change("tempo", value)}
        />
        <Check
          id="midi-meter"
          label={t("Import the first time signature")}
          checked={options.timeSignature}
          disabled={importing}
          onChange={(value) => change("timeSignature", value)}
        />
        <Check
          id="midi-mix"
          label={t("Import channel volume and pan")}
          checked={options.channelMix}
          disabled={importing}
          onChange={(value) => change("channelMix", value)}
        />
        <Check id="midi-note-expression" label={t("Import pitch bend and controllers as note expression")} checked={options.noteExpression ?? true} disabled={importing} onChange={(value) => change("noteExpression", value)} />
      </FieldSet>
      <div role="status" aria-live="polite">
        {reading && t("Reading MIDI…")}
        {preview && (
          <>
            <p>
              {message("MIDI import summary", { channels: message("MIDI channels", { count: preview.channels.length }), patterns: message("MIDI patterns", { count: preview.patterns }), clips: message("MIDI clips", { count: preview.clips }), notes: message("MIDI notes", { count: preview.notes }) })}
            </p>
            {preview.channels.length > 0 && (
              <p>{preview.channels.join(", ")}</p>
            )}
            {preview.adjustments.length > 0 && (
              <ul className="list-disc pl-4">
                {preview.adjustments.map((line, index) => (
                  <li key={index}>{localizeBackendText(line)}</li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>
      {error && <FieldError role="alert">{localizeBackendText(error)}</FieldError>}
      <DialogFooter>
        <Button variant="outline" disabled={importing} onClick={done}>{t("Cancel")}</Button>
        <Button
          disabled={!preview || reading || importing}
          onClick={() => void append()}
        >
          {importing ? t("Importing…") : t("Import")}
        </Button>
      </DialogFooter>
    </FieldGroup>
  )
}

function ExportForm({ done, busy }: FormProps) {
  useLanguageStore((state) => state.language)
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
      toast.success(t("Exported MIDI"), { description: fileName(saved) })
      done()
    } catch (failure) {
      setError(errorSource(failure))
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
        <FieldLabel htmlFor="midi-mode">{t("Export")}</FieldLabel>
        <Choice
          id="midi-mode"
          value={options.mode}
          disabled={working}
          options={[
            { value: "pattern", label: t("The selected pattern") },
            { value: "song", label: t("The whole song") },
          ]}
          onChange={(mode) => setOptions({ ...options, mode })}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-format">{t("MIDI format")}</FieldLabel>
        <Choice
          id="midi-format"
          value={options.singleTrack ? "single" : "multi"}
          disabled={working}
          options={[
            { value: "multi", label: t("Format 1 — separate tracks") },
            { value: "single", label: t("Format 0 — one track") },
          ]}
          onChange={(value) =>
            setOptions({ ...options, singleTrack: value === "single" })
          }
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-ppq">{t("Resolution")}</FieldLabel>
        <Choice
          id="midi-ppq"
          value={options.ppq}
          disabled={working}
          options={PROJECT_TIMEBASES.map((value) => ({ value, label: message("{value} PPQ", { value }) }))}
          onChange={(ppq) => { setOptions({ ...options, ppq }); setResolutionText(String(ppq)) }}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="midi-custom-ppq">{t("Custom MIDI resolution (1–32767)")}</FieldLabel>
        <Input id="midi-custom-ppq" type="number" min={1} max={32767} value={resolutionText} aria-invalid={!resolutionValid} disabled={working} onChange={(event) => { const text = event.target.value; setResolutionText(text); const ppq = Number(text); if (text.trim() && Number.isInteger(ppq) && ppq >= 1 && ppq <= 32767) setOptions({ ...options, ppq }) }} />
        {!resolutionValid && <FieldError>{t("Use a whole resolution from 1 to 32767 PPQ.")}</FieldError>}
      </Field>
      <FieldSet>
        <FieldLegend>{t("Playback")}</FieldLegend>
        <Check
          id="midi-swing"
          label={t("Apply project swing")}
          checked={options.swing}
          disabled={working}
          onChange={(swing) => setOptions({ ...options, swing })}
        />
        <Check
          id="midi-running"
          label={t("Use running status (smaller file)")}
          checked={options.runningStatus}
          disabled={working}
          onChange={(runningStatus) =>
            setOptions({ ...options, runningStatus })
          }
        />
      </FieldSet>
      <FieldSet><FieldLegend>{t("Note expression")}</FieldLegend>
        <Field><FieldLabel htmlFor="midi-expression-mode">{t("Expression routing")}</FieldLabel>
          <Choice<MidiExpressionMode> id="midi-expression-mode" value={expression.mode} disabled={working}
            options={[{ value: "off", label: t("Notes only") }, { value: "channelVoice", label: t("Color-group channels") }, { value: "mpe", label: t("MPE · one channel per note") }]}
            onChange={(mode) => setOptions({ ...options, expression: { ...expression, mode } })} /></Field>
        <Field><FieldLabel htmlFor="midi-expression-bend">{t("Pitch-bend range (semitones; 0 = automatic)")}</FieldLabel>
          <Input id="midi-expression-bend" type="number" min={0} max={96} value={Number.isFinite(expression.bendRange) ? expression.bendRange : ""} disabled={working || expression.mode === "off"}
            onChange={(event) => setOptions({ ...options, expression: { ...expression, bendRange: event.target.valueAsNumber } })} /></Field>
        <Field><FieldLabel htmlFor="midi-expression-step">{t("Expression spacing (ticks; 0 = automatic)")}</FieldLabel>
          <Input id="midi-expression-step" type="number" min={0} max={245760} value={Number.isFinite(expression.step) ? expression.step : ""} disabled={working || expression.mode === "off"}
            onChange={(event) => setOptions({ ...options, expression: { ...expression, step: event.target.valueAsNumber } })} /></Field>
        <FieldDescription>{t("Color-group channels share bends and controllers. MPE assigns member channels and overrides note colors. Pitch bend, modulation, pan and saved curves are sampled; match the receiving instrument’s bend range.")}</FieldDescription>
      </FieldSet>
      <FieldDescription>
        {t("Exports notes, tempo and meter. Audio clips, instrument sounds and effects are omitted. Tempo glides are sampled; more than 15 melodic tracks share MIDI channels.")}
      </FieldDescription>
      {error && <FieldError role="alert">{localizeBackendText(error)}</FieldError>}
      <DialogFooter>
        <Button variant="outline" disabled={working} onClick={done}>{t("Cancel")}</Button>
        <Button disabled={working || !expressionValid || !resolutionValid} onClick={() => void save()}>
          {working ? t("Exporting…") : t("Choose file and export…")}
        </Button>
      </DialogFooter>
    </FieldGroup>
  )
}

export function MidiDialogs() {
  useLanguageStore((state) => state.language)
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
          <DialogTitle>{importing ? t("Import MIDI") : t("Export MIDI")}</DialogTitle>
          <DialogDescription>
            {importing
              ? t("Append notes and arrangement to this project as one undo step. MIDI instruments use the subtractive synth unless drums are mapped.")
              : t("Save notes and tempo as a MIDI file. This exports no audio.")}
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
