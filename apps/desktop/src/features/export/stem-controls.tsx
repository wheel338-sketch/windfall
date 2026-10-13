import type { ExportStems } from "@/bindings"
import { Checkbox } from "@/components/ui/checkbox"
import { Button } from "@/components/ui/button"
import { selectedMixerTracks, useMixerUi } from "@/features/mixer/mixer-ui"
import { useUiStore } from "@/lib/store/ui"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { useProjectStore } from "@/lib/store/project"
import { MASTER_TRACK } from "@/lib/units"

import { Choice } from "./choice"
import { translatedText as t } from "@/lib/i18n"

const DEFAULT_STEMS: ExportStems = {
  mode: "trackOutputs",
  includeMix: true,
  numbered: true,
  folder: true,
}

export function StemControls({
  value,
  disabled,
  onChange,
}: {
  value: ExportStems | undefined
  disabled: boolean
  onChange(value: ExportStems | undefined): void
}) {
  const mixer = useProjectStore((state) => state.project.mixer)
  const tracks = mixer.tracks.filter((track) => track.id !== MASTER_TRACK && !track.current)
  const selection = useMixerUi((state) => state.selected)
  const primary = useUiStore((state) => state.selectedTrack)
  const selected = tracks.filter((track) => (selection.length ? selection : [primary]).includes(track.id))
  const armed = tracks.filter((track) => track.recording?.armed)
  const set = <K extends keyof ExportStems>(key: K, next: ExportStems[K]) => {
    if (value) onChange({ ...value, [key]: next })
  }
  return (
    <FieldSet disabled={disabled}>
      <FieldLegend variant="label">{t("Files to export")}</FieldLegend>
      <FieldGroup className="gap-3">
        <Field orientation="horizontal" data-disabled={disabled || undefined}>
          <Checkbox
            id="export-stems"
            checked={value !== undefined}
            disabled={disabled}
            onCheckedChange={(checked) =>
              onChange(checked ? DEFAULT_STEMS : undefined)
            }
          />
          <FieldLabel htmlFor="export-stems">
            {t("Export mixer tracks as stems")}
          </FieldLabel>
        </Field>
        {value && (
          <>
            <Field data-disabled={disabled || undefined}>
              <FieldLabel htmlFor="export-stem-mode">
                {t("Each stem contains")}
              </FieldLabel>
              <Choice
                id="export-stem-mode"
                value={value.mode}
                options={[
                  { value: "trackOutputs", label: t("The track output") },
                  { value: "toMaster", label: t("The source through the master") },
                ]}
                disabled={disabled}
                onChange={(mode) => set("mode", mode)}
              />
              <FieldDescription>
                {value.mode === "trackOutputs"
                  ? t("After track effects and faders. Buses are included; their stems can overlap with the tracks feeding them.")
                  : t("Includes bus and master processing. Compressors and limiters react to each stem separately.")}
              </FieldDescription>
            </Field>
            <Field data-disabled={disabled || undefined}>
              <FieldLabel htmlFor="export-stem-selection">
                {t("Mixer tracks")}
              </FieldLabel>
              <Choice
                id="export-stem-selection"
                value={value.tracks === undefined ? "all" : "selected"}
                options={[
                  { value: "all", label: t("All tracks that have sound") },
                  { value: "selected", label: t("Choose tracks") },
                ]}
                disabled={disabled}
                onChange={(selection) =>
                  set(
                    "tracks",
                    selection === "all"
                      ? undefined
                      : tracks.map((track) => track.id)
                  )
                }
              />
            </Field>
            <div className="flex flex-wrap gap-2">
              <Button type="button" size="sm" variant="outline" disabled={disabled || !selected.length} onClick={() => set("tracks", selectedMixerTracks().filter((track) => track.id !== MASTER_TRACK && !track.current).map((track) => track.id))}>{t("Use mixer selection (")}{selected.length})</Button>
              <Button type="button" size="sm" variant="outline" disabled={disabled || !armed.length} onClick={() => set("tracks", tracks.filter((track) => track.recording?.armed).map((track) => track.id))}>{t("Use armed tracks (")}{armed.length})</Button>
              {value.tracks !== undefined && <Button type="button" size="sm" variant="ghost" disabled={disabled} onClick={() => set("tracks", [])}>{t("Clear track selection")}</Button>}
            </div>
            {value.tracks !== undefined && (
              <FieldSet>
                <FieldLegend variant="label">{t("Selected tracks")}</FieldLegend>
                <FieldGroup className="max-h-36 gap-2 overflow-y-auto">
                  {tracks.map((track) => (
                    <Field
                      key={track.id}
                      orientation="horizontal"
                      data-disabled={disabled || undefined}
                    >
                      <Checkbox
                        id={`export-track-${track.id}`}
                        checked={value.tracks?.includes(track.id)}
                        disabled={disabled}
                        onCheckedChange={(checked) =>
                          set(
                            "tracks",
                            checked
                              ? [...(value.tracks ?? []), track.id]
                              : value.tracks?.filter((id) => id !== track.id)
                          )
                        }
                      />
                      <FieldLabel htmlFor={`export-track-${track.id}`}>
                        {track.name}
                      </FieldLabel>
                    </Field>
                  ))}
                </FieldGroup>
              </FieldSet>
            )}
            <FieldGroup className="gap-2">
              <Field
                orientation="horizontal"
                data-disabled={disabled || undefined}
              >
                <Checkbox
                  id="export-stem-mix"
                  checked={value.includeMix}
                  disabled={disabled}
                  onCheckedChange={(checked) => set("includeMix", checked)}
                />
                <FieldLabel htmlFor="export-stem-mix">
                  {t("Include the full mix")}
                </FieldLabel>
              </Field>
              <Field
                orientation="horizontal"
                data-disabled={disabled || undefined}
              >
                <Checkbox
                  id="export-stem-numbered"
                  checked={value.numbered}
                  disabled={disabled}
                  onCheckedChange={(checked) => set("numbered", checked)}
                />
                <FieldLabel htmlFor="export-stem-numbered">
                  {t("Number files in mixer order")}
                </FieldLabel>
              </Field>
              <Field
                orientation="horizontal"
                data-disabled={disabled || undefined}
              >
                <Checkbox
                  id="export-stem-folder"
                  checked={value.folder}
                  disabled={disabled}
                  onCheckedChange={(checked) => set("folder", checked)}
                />
                <FieldLabel htmlFor="export-stem-folder">
                  {t("Put stems in their own folder")}
                </FieldLabel>
              </Field>
            </FieldGroup>
          </>
        )}
      </FieldGroup>
    </FieldSet>
  )
}
