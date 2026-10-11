import type { MasteringReview } from "@/bindings"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"

import { Choice } from "./choice"
import {
  CUSTOM_PRESET,
  DYNAMICS_CHOICES,
  MASTERING_CEILING,
  MASTERING_PRESETS,
  MASTERING_TARGET,
  presetOf,
  reviewRows,
  reviewSummary,
  reviewVerdict,
  type MasteringDraft,
  type MasteringRequest,
} from "./mastering"

const PRESET_OPTIONS = [
  ...MASTERING_PRESETS.map(({ id, label }) => ({ value: id, label })),
  { value: CUSTOM_PRESET, label: "Custom" },
]

/**
 * Mastering on export: whether the mix is brought to a loudness target, and
 * to which. It is done on this computer while the file is written.
 */
export function MasteringControls({
  value,
  request,
  disabled,
  onChange,
}: {
  value: MasteringDraft
  request: MasteringRequest
  disabled: boolean
  onChange(value: MasteringDraft): void
}) {
  const set = <K extends keyof MasteringDraft>(key: K, next: MasteringDraft[K]) =>
    onChange({ ...value, [key]: next })
  return (
    <FieldSet disabled={disabled} data-slot="mastering-controls">
      <FieldLegend variant="label">Mastering</FieldLegend>
      <FieldGroup className="gap-3">
        <Field orientation="horizontal" data-disabled={disabled || undefined}>
          <Checkbox
            id="export-mastering"
            checked={value.enabled}
            disabled={disabled}
            onCheckedChange={(enabled) => set("enabled", enabled)}
          />
          <FieldLabel htmlFor="export-mastering">
            Master the mix to a loudness target
          </FieldLabel>
        </Field>
        {value.enabled && (
          <FieldDescription>
            Targets and review measure the stereo master before encoding.
            WAV float preserves those samples; lossy encoding or mono conversion
            can change the final levels.
          </FieldDescription>
        )}
        {value.enabled && (
          <>
            <FieldDescription>
              The mix is measured, evened out and limited on this computer,
              then measured again. Nothing is uploaded.
            </FieldDescription>
            <FieldGroup className="grid grid-cols-2 gap-3">
              <Field className="col-span-2" data-disabled={disabled || undefined}>
                <FieldLabel htmlFor="export-mastering-preset">
                  Master for
                </FieldLabel>
                <Choice
                  id="export-mastering-preset"
                  value={presetOf(value)}
                  options={PRESET_OPTIONS}
                  disabled={disabled}
                  onChange={(id) => {
                    const preset = MASTERING_PRESETS.find(
                      (item) => item.id === id
                    )
                    if (preset) {
                      onChange({
                        ...value,
                        target: preset.target,
                        ceiling: preset.ceiling,
                      })
                    }
                  }}
                />
              </Field>
              <Field data-invalid={request.targetError !== null || undefined}>
                <FieldLabel htmlFor="export-mastering-target">
                  Loudness target (LUFS)
                </FieldLabel>
                <Input
                  id="export-mastering-target"
                  type="number"
                  min={MASTERING_TARGET.min}
                  max={MASTERING_TARGET.max}
                  step={0.5}
                  value={value.target}
                  disabled={disabled}
                  aria-invalid={request.targetError !== null || undefined}
                  aria-describedby={
                    request.targetError ? "export-mastering-target-error" : undefined
                  }
                  onChange={(event) => set("target", event.target.value)}
                />
                {request.targetError && (
                  <FieldError id="export-mastering-target-error">
                    {request.targetError}
                  </FieldError>
                )}
              </Field>
              <Field data-invalid={request.ceilingError !== null || undefined}>
                <FieldLabel htmlFor="export-mastering-ceiling">
                  True-peak ceiling (dBTP)
                </FieldLabel>
                <Input
                  id="export-mastering-ceiling"
                  type="number"
                  min={MASTERING_CEILING.min}
                  max={MASTERING_CEILING.max}
                  step={0.1}
                  value={value.ceiling}
                  disabled={disabled}
                  aria-invalid={request.ceilingError !== null || undefined}
                  aria-describedby={
                    request.ceilingError
                      ? "export-mastering-ceiling-error"
                      : undefined
                  }
                  onChange={(event) => set("ceiling", event.target.value)}
                />
                {request.ceilingError && (
                  <FieldError id="export-mastering-ceiling-error">
                    {request.ceilingError}
                  </FieldError>
                )}
              </Field>
              <Field className="col-span-2" data-disabled={disabled || undefined}>
                <FieldLabel htmlFor="export-mastering-dynamics">
                  Dynamics
                </FieldLabel>
                <Choice
                  id="export-mastering-dynamics"
                  value={value.dynamics}
                  options={[...DYNAMICS_CHOICES]}
                  disabled={disabled}
                  onChange={(dynamics) => set("dynamics", dynamics)}
                />
                <FieldDescription>
                  How firmly three band compressors hold the lows, the middle
                  and the highs together before the limiter.
                </FieldDescription>
              </Field>
            </FieldGroup>
            {request.stemsError && (
              <FieldError id="export-mastering-stems-error">
                {request.stemsError}
              </FieldError>
            )}
          </>
        )}
      </FieldGroup>
    </FieldSet>
  )
}

/** What mastering measured before and after, once the file is written. */
export function MasteringReviewPanel({ review }: { review: MasteringReview }) {
  const rows = reviewRows(review)
  return (
    <Alert
      data-mastering-review=""
      variant={review.targetReached ? "default" : "destructive"}
    >
      <AlertTitle>Mastering review</AlertTitle>
      <AlertDescription>
        <p data-slot="mastering-verdict">{reviewVerdict(review)}</p>
        <table className="mt-2 w-full text-left font-readout text-xs">
          <caption className="sr-only">
            The mix before mastering and the master that was written
          </caption>
          <thead>
            <tr className="text-muted-foreground">
              <th scope="col" className="py-0.5 pr-2 font-normal">
                Measure
              </th>
              <th scope="col" className="py-0.5 pr-2 font-normal">
                Before
              </th>
              <th scope="col" className="py-0.5 pr-2 font-normal">
                After
              </th>
              <th scope="col" className="py-0.5 font-normal">
                Change
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((item) => (
              <tr key={item.id} data-measure={item.id}>
                <th scope="row" className="py-0.5 pr-2 font-normal">
                  {item.label}
                </th>
                <td className="py-0.5 pr-2">{item.before}</td>
                <td className="py-0.5 pr-2 text-foreground">{item.after}</td>
                <td className="py-0.5">{item.change}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="mt-2" data-slot="mastering-summary">
          {reviewSummary(review)}
        </p>
      </AlertDescription>
    </Alert>
  )
}
