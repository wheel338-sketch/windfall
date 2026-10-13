import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useId, useRef, useState } from "react"
import type { PluginBinding, PluginWrapperOptions } from "@/bindings"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { dispatch, useProjectGeneration, useProjectStore } from "@/lib/store"
import { findPluginBinding } from "./store"

const DEFAULTS: PluginWrapperOptions = {
  smartDisable: false,
  processing: "automatic",
  editorScale: 1,
  detached: true,
}
const BUFFER_SIZES = [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192]
const EDITOR_SCALES = [0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4]

function NumericChoice({
  label,
  value,
  items,
  disabled,
  change,
}: {
  label: string
  value: number
  items: { value: number; label: string }[]
  disabled: boolean
  change(value: number): void
}) {
  useLanguageStore((state) => state.language)
  const id = useId()
  return (
    <Field>
      <FieldLabel htmlFor={id}>{t(label)}</FieldLabel>
      <Select
        value={value}
        items={items}
        disabled={disabled}
        onValueChange={(next) => {
          if (next !== null) change(next)
        }}
      >
        <SelectTrigger id={id} aria-label={t(label)}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          <SelectGroup>
            {items.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {t(item.label)}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

function WrapperFields({ binding }: { binding: PluginBinding }) {
  useLanguageStore((state) => state.language)
  const smartId = useId()
  const detachedId = useId()
  const processingId = useId()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const working = useRef(false)
  const options = binding.wrapper ?? DEFAULTS
  const buffer = options.fixedBuffer ?? 0
  const buffers = [
    { value: 0, label: t("Variable") },
    ...BUFFER_SIZES.map((value) => ({ value, label: i18nMessage("{value} frames", { value }) })),
    ...(buffer !== 0 && !BUFFER_SIZES.includes(buffer)
      ? [{ value: buffer, label: i18nMessage("{value0} frames (saved)", { value0: buffer }) }]
      : []),
  ]
  const scales = [
    ...EDITOR_SCALES.map((value) => ({ value, label: `${value * 100}%` })),
    ...(!EDITOR_SCALES.includes(options.editorScale)
      ? [{ value: options.editorScale, label: i18nMessage("{value}% (saved)", { value: options.editorScale * 100 }) }]
      : []),
  ]

  async function change(patch: Partial<PluginWrapperOptions>) {
    if (working.current) return
    // Read the current saved policy so an unrelated edit cannot be overwritten
    // by controls rendered from an older binding. No optimistic DSP state.
    const current = findPluginBinding(useProjectStore.getState().project, binding.target)
    if (!current || current.format !== binding.format || current.path !== binding.path || current.id !== binding.id) return
    working.current = true
    setBusy(true)
    setError(null)
    try {
      const result = await dispatch({
        type: "setPluginWrapper",
        target: current.target,
        options: { ...(current.wrapper ?? DEFAULTS), ...patch },
      })
      if (!result) setError(t("Wrapper settings were not changed. Try again."))
    } finally {
      working.current = false
      setBusy(false)
    }
  }

  return (
    <FieldSet disabled={busy} aria-busy={busy}>
      <FieldLegend>{t("Plugin wrapper")}</FieldLegend>
      <FieldDescription>
        {t("Settings are saved with this plugin. Processing changes restart its audio; editor settings apply the next time its window opens.")}
      </FieldDescription>
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor={smartId}>{t("Smart disable")}</FieldLabel>
            <FieldDescription>
              {t("Sleep after a finite tail and sustained silence. Input or controls wake the plugin; plugins with endless tails stay awake.")}
            </FieldDescription>
          </FieldContent>
          <Switch
            id={smartId}
            checked={options.smartDisable}
            disabled={busy}
            onCheckedChange={(smartDisable) => void change({ smartDisable })}
          />
        </Field>
        <NumericChoice
          label={t("Native buffer")}
          value={buffer}
          items={buffers}
          disabled={busy}
          change={(fixedBuffer) => void change({ fixedBuffer: fixedBuffer || undefined })}
        />
        <FieldDescription>{t("A fixed buffer adds that many samples of latency. Choose Variable to process the incoming block size.")}</FieldDescription>
        <Field>
          <FieldLabel id={processingId}>{t("Processing")}</FieldLabel>
          <ToggleGroup
            aria-labelledby={processingId}
            value={[options.processing]}
            disabled={busy}
            className="flex-wrap"
            variant="outline"
            size="sm"
            onValueChange={(value) => {
              const processing = value[0]
              if (
                processing === "automatic" ||
                processing === "inProcess" ||
                processing === "threaded" ||
                processing === "bridged"
              ) void change({ processing })
            }}
          >
            <ToggleGroupItem value="automatic">{t("Automatic")}</ToggleGroupItem>
            <ToggleGroupItem value="inProcess">{t("In process")}</ToggleGroupItem>
            <ToggleGroupItem value="bridged">{t("Bridged")}</ToggleGroupItem>
            <ToggleGroupItem value="threaded">{t("Threaded")}</ToggleGroupItem>
          </ToggleGroup>
          <FieldDescription>{t("Automatic uses the platform default. Bridged isolates failures in a helper process. Threaded uses a dedicated worker and adds two native blocks of latency. A missed worker deadline reports a plugin failure. In process shares the desktop process.")}</FieldDescription>
        </Field>
        <NumericChoice
          label={t("Native editor scale")}
          value={options.editorScale}
          items={scales}
          disabled={busy}
          change={(editorScale) => void change({ editorScale })}
        />
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor={detachedId}>{t("Detached editor")}</FieldLabel>
            <FieldDescription>{t("Turn off to keep an editor window owned by Windfall. Scaling and window ownership require support from the native plugin.")}</FieldDescription>
          </FieldContent>
          <Switch
            id={detachedId}
            checked={options.detached}
            disabled={busy}
            onCheckedChange={(detached) => void change({ detached })}
          />
        </Field>
      </FieldGroup>
      {error && <FieldError role="alert">{error}</FieldError>}
    </FieldSet>
  )
}

export function PluginWrapperControls({ binding }: { binding: PluginBinding }) {
  useLanguageStore((state) => state.language)
  const generation = useProjectGeneration()
  return (
    <WrapperFields
      key={`${generation}:${JSON.stringify(binding.target)}:${binding.format}:${binding.path}:${binding.id}`}
      binding={binding}
    />
  )
}
