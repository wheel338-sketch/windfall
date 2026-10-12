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
import { sameTarget } from "./store"

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
  const id = useId()
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Select
        value={value}
        items={items}
        disabled={disabled}
        onValueChange={(next) => {
          if (next !== null) change(next)
        }}
      >
        <SelectTrigger id={id} aria-label={label}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          <SelectGroup>
            {items.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

function WrapperFields({ binding }: { binding: PluginBinding }) {
  const smartId = useId()
  const detachedId = useId()
  const processingId = useId()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const working = useRef(false)
  const options = binding.wrapper ?? DEFAULTS
  const buffer = options.fixedBuffer ?? 0
  const buffers = [
    { value: 0, label: "Variable" },
    ...BUFFER_SIZES.map((value) => ({ value, label: `${value} frames` })),
    ...(buffer !== 0 && !BUFFER_SIZES.includes(buffer)
      ? [{ value: buffer, label: `${buffer} frames (saved)` }]
      : []),
  ]
  const scales = [
    ...EDITOR_SCALES.map((value) => ({ value, label: `${value * 100}%` })),
    ...(!EDITOR_SCALES.includes(options.editorScale)
      ? [{ value: options.editorScale, label: `${options.editorScale * 100}% (saved)` }]
      : []),
  ]

  async function change(patch: Partial<PluginWrapperOptions>) {
    if (working.current) return
    // Read the current saved policy so an unrelated edit cannot be overwritten
    // by controls rendered from an older binding. No optimistic DSP state.
    const current = useProjectStore.getState().project.plugins?.find(
      (candidate) =>
        sameTarget(candidate.target, binding.target) &&
        candidate.format === binding.format &&
        candidate.path === binding.path &&
        candidate.id === binding.id
    )
    if (!current) return
    working.current = true
    setBusy(true)
    setError(null)
    try {
      const result = await dispatch({
        type: "setPluginWrapper",
        target: current.target,
        options: { ...(current.wrapper ?? DEFAULTS), ...patch },
      })
      if (!result) setError("Wrapper settings were not changed. Try again.")
    } finally {
      working.current = false
      setBusy(false)
    }
  }

  return (
    <FieldSet disabled={busy} aria-busy={busy}>
      <FieldLegend>Plugin wrapper</FieldLegend>
      <FieldDescription>
        Settings are saved with this plugin. Processing changes restart its audio;
        editor settings apply the next time its window opens.
      </FieldDescription>
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor={smartId}>Smart disable</FieldLabel>
            <FieldDescription>
              Sleep after a finite tail and sustained silence. Input or controls
              wake the plugin; plugins with endless tails stay awake.
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
          label="Native buffer"
          value={buffer}
          items={buffers}
          disabled={busy}
          change={(fixedBuffer) => void change({ fixedBuffer: fixedBuffer || undefined })}
        />
        <FieldDescription>
          A fixed buffer adds that many samples of latency. Choose Variable to
          process the incoming block size.
        </FieldDescription>
        <Field>
          <FieldLabel id={processingId}>Processing</FieldLabel>
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
            <ToggleGroupItem value="automatic">Automatic</ToggleGroupItem>
            <ToggleGroupItem value="inProcess">In process</ToggleGroupItem>
            <ToggleGroupItem value="bridged">Bridged</ToggleGroupItem>
            <ToggleGroupItem value="threaded">Threaded</ToggleGroupItem>
          </ToggleGroup>
          <FieldDescription>
            Automatic uses the platform default. Bridged isolates failures in a
            helper process. Threaded uses a dedicated worker and adds two native
            blocks of latency. A missed worker deadline reports a plugin failure.
            In process shares the desktop process.
          </FieldDescription>
        </Field>
        <NumericChoice
          label="Native editor scale"
          value={options.editorScale}
          items={scales}
          disabled={busy}
          change={(editorScale) => void change({ editorScale })}
        />
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor={detachedId}>Detached editor</FieldLabel>
            <FieldDescription>
              Turn off to keep an editor window owned by Windfall. Scaling and
              window ownership require support from the native plugin.
            </FieldDescription>
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
  const generation = useProjectGeneration()
  return (
    <WrapperFields
      key={`${generation}:${JSON.stringify(binding.target)}:${binding.format}:${binding.path}:${binding.id}`}
      binding={binding}
    />
  )
}
