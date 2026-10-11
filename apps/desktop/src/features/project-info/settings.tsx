import { useId } from "react"
import type { PanLaw } from "@/bindings"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Choice } from "@/features/export/choice"
import { dispatch, useProjectStore } from "@/lib/store/project"

export const PROJECT_TIMEBASES = [24, 48, 96, 120, 192, 240, 480, 960]
export function ProjectPlaybackSettings() {
  const settings = useProjectStore((state) => state.project.settings)
  const id = useId()
  return <FieldGroup>
    <Field><FieldLabel htmlFor={`${id}-law`}>Panning law</FieldLabel>
      <Choice<PanLaw> disabled={false} id={`${id}-law`} value={settings.panLaw ?? "balance"} options={[{ value: "balance", label: "Balance (legacy)" }, { value: "constantPower", label: "Constant power (+3 dB)" }, { value: "compromise", label: "Compromise (+4.5 dB)" }, { value: "linear", label: "Linear (+6 dB)" }]} onChange={(panLaw) => void dispatch({ type: "updateSettings", patch: { panLaw } })} />
      <FieldDescription>Unity at center. Applies to channel, clip and mixer panning, and instruments that support per-note pan.</FieldDescription>
    </Field>
    <Field><FieldLabel htmlFor={`${id}-ppq`}>Project timebase</FieldLabel>
      <Choice<number> disabled={false} id={`${id}-ppq`} value={settings.timebasePpq ?? 960} options={PROJECT_TIMEBASES.map((value) => ({ value, label: `${value} PPQ` }))} onChange={(timebasePpq) => void dispatch({ type: "updateSettings", patch: { timebasePpq } })} />
      <FieldDescription>Playback and export use this grid. Stored note positions stay at 960 PPQ.</FieldDescription>
    </Field>
  </FieldGroup>
}
