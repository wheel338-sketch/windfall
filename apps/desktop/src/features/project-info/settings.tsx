import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { NativeNoteCapabilities } from "@/features/plugins/note-capabilities-view"
import { useId } from "react"
import type { PanLaw } from "@/bindings"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Choice } from "@/features/export/choice"
import { dispatch, useProjectStore } from "@/lib/store/project"

export const PROJECT_TIMEBASES = [24, 48, 96, 120, 192, 240, 480, 960]
export function ProjectPlaybackSettings() {
  useLanguageStore((state) => state.language)
  const settings = useProjectStore((state) => state.project.settings)
  const plugins = useProjectStore((state) => state.project.plugins)
  const id = useId()
  return <FieldGroup>
    <Field><FieldLabel htmlFor={`${id}-law`}>{t("Panning law")}</FieldLabel>
      <Choice<PanLaw> disabled={false} id={`${id}-law`} value={settings.panLaw ?? "balance"} options={[{ value: "balance", label: t("Balance (legacy)") }, { value: "constantPower", label: t("Constant power (+3 dB)") }, { value: "compromise", label: t("Compromise (+4.5 dB)") }, { value: "linear", label: t("Linear (+6 dB)") }]} onChange={(panLaw) => void dispatch({ type: "updateSettings", patch: { panLaw } })} />
      <FieldDescription>{t("Unity at center. Applies to channel, clip and mixer panning, and instruments that support per-note pan.")}</FieldDescription>
    </Field>
    <Field><FieldLabel htmlFor={`${id}-ppq`}>{t("Project timebase")}</FieldLabel>
      <Choice<number> disabled={false} id={`${id}-ppq`} value={settings.timebasePpq ?? 960} options={PROJECT_TIMEBASES.map((value) => ({ value, label: message("{value} PPQ", { value }) }))} onChange={(timebasePpq) => void dispatch({ type: "updateSettings", patch: { timebasePpq } })} />
      <FieldDescription>{t("Playback and export use this grid. Stored note positions stay at 960 PPQ.")}</FieldDescription>
    </Field>
    {(plugins ?? []).flatMap((binding) => binding.target.type === "instrument" ? [<NativeNoteCapabilities key={binding.target.channel} binding={binding} />] : [])}
  </FieldGroup>
}
