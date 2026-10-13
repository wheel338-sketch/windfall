import { useMemo, type ReactNode } from "react"

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Toggle } from "@/components/ui/toggle"
import { cn } from "@/lib/utils"

import { SCOPE_SLOT, useAnalyzerSelection } from "./feed"
import {
  closeScope,
  SCOPE_FLOORS,
  SCOPE_SPANS,
  SCOPE_VIEW_NAMES,
  SCOPE_VIEWS,
  scopeWant,
  setScopeSource,
  setScopeView,
  useScopeStore,
  type ScopeView,
} from "./scope-store"
import { AnalyzerSourceSelect } from "./source-select"
import { AnalyzerStatusLine } from "./status"
import {
  Oscilloscope,
  PeakMeter,
  Spectrogram,
  SpectrumCurve,
  Vectorscope,
} from "./views"
import { translatedText as t } from "@/lib/i18n"

type ChoiceProps<Value extends number> = {
  label: string
  values: readonly Value[]
  value: Value
  format(value: Value): string
  onChange(value: Value): void
}

/** A short row of exclusive choices, moved through with the arrow keys. */
function Choice<Value extends number>({
  label,
  values,
  value,
  format,
  onChange,
}: ChoiceProps<Value>) {
  const index = values.indexOf(value)
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="flex shrink-0 rounded-[5px] bg-(--wf-step-off)/70 p-px"
      onKeyDown={(event) => {
        const step =
          event.key === "ArrowRight" || event.key === "ArrowDown"
            ? 1
            : event.key === "ArrowLeft" || event.key === "ArrowUp"
              ? -1
              : 0
        if (step === 0) return
        event.preventDefault()
        const next = values[(index + step + values.length) % values.length]
        onChange(next)
        event.currentTarget
          .querySelector<HTMLElement>(`[data-value="${next}"]`)
          ?.focus()
      }}
    >
      {values.map((item) => (
        <button
          key={item}
          type="button"
          role="radio"
          aria-checked={item === value}
          data-value={item}
          tabIndex={item === value ? 0 : -1}
          onClick={() => onChange(item)}
          className={cn(
            "h-5 rounded-[4px] px-1.5 font-readout text-[9px] leading-none text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
            item === value && "bg-background text-foreground shadow-xs"
          )}
        >
          {format(item)}
        </button>
      ))}
    </div>
  )
}

function ViewFrame({
  view,
  children,
  className,
}: {
  view: ScopeView
  children: ReactNode
  className?: string
}) {
  return (
    <section
      aria-label={SCOPE_VIEW_NAMES[view]}
      data-slot="scope-view"
      data-view={view}
      className={cn("flex min-w-0 flex-col gap-1", className)}
    >
      <h3 className="text-[0.6875rem] leading-none font-medium text-foreground/80">
        {SCOPE_VIEW_NAMES[view]}
      </h3>
      <div className="relative h-36 overflow-hidden rounded-[5px] bg-(--wf-meter-bg) ring-1 ring-(--wf-grid-line-strong)">
        {children}
      </div>
    </section>
  )
}

/**
 * The scope while it is open. It owns the scope's analyzer slot: the source
 * picked here is what every view below draws, and only the parts the shown
 * views need are asked for.
 */
function ScopeBody() {
  const source = useScopeStore((state) => state.source)
  const views = useScopeStore((state) => state.views)
  const span = useScopeStore((state) => state.span)
  const floorDb = useScopeStore((state) => state.floorDb)
  const triggered = useScopeStore((state) => state.triggered)
  const selection = useMemo(
    () => ({ source, want: scopeWant(views) }),
    [source, views]
  )
  useAnalyzerSelection(SCOPE_SLOT, selection)
  const shows = (view: ScopeView) => views.includes(view)

  return (
    <div data-slot="signal-scope" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <AnalyzerSourceSelect source={source} onChange={setScopeSource} />
        <AnalyzerStatusLine slot={SCOPE_SLOT} className="flex-1" />
      </div>
      <div
        role="group"
        aria-label={t("Views")}
        className="flex flex-wrap items-center gap-1"
      >
        {SCOPE_VIEWS.map((view) => (
          <Toggle
            key={view}
            size="sm"
            variant="outline"
            pressed={shows(view)}
            onPressedChange={(pressed) => setScopeView(view, pressed)}
          >
            {SCOPE_VIEW_NAMES[view]}
          </Toggle>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-3 text-[10px] text-muted-foreground">
        <span className="flex items-center gap-1.5">
          {t("Oscilloscope width")}
          <Choice
            label={t("Samples the oscilloscope shows")}
            values={SCOPE_SPANS}
            value={span}
            format={(value) => String(value)}
            onChange={(value) => useScopeStore.setState({ span: value })}
          />
        </span>
        <Toggle
          size="sm"
          variant="outline"
          pressed={triggered}
          onPressedChange={(pressed) =>
            useScopeStore.setState({ triggered: pressed })
          }
        >
          {t("Hold steady")}
        </Toggle>
        <span className="flex items-center gap-1.5">
          {t("Floor")}
          <Choice
            label={t("Quietest level shown")}
            values={SCOPE_FLOORS}
            value={floorDb}
            format={(value) => `${value} dB`}
            onChange={(value) => useScopeStore.setState({ floorDb: value })}
          />
        </span>
      </div>
      {views.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted-foreground">
          {t("Every view is hidden. Pick one above.")}
        </p>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {shows("meter") && (
            <ViewFrame view="meter" className="sm:col-span-2">
              <PeakMeter slot={SCOPE_SLOT} floorDb={floorDb} />
            </ViewFrame>
          )}
          {shows("oscilloscope") && (
            <ViewFrame view="oscilloscope">
              <Oscilloscope
                slot={SCOPE_SLOT}
                span={span}
                triggered={triggered}
              />
            </ViewFrame>
          )}
          {shows("spectrum") && (
            <ViewFrame view="spectrum">
              <SpectrumCurve slot={SCOPE_SLOT} floorDb={floorDb} />
            </ViewFrame>
          )}
          {shows("spectrogram") && (
            <ViewFrame view="spectrogram">
              <Spectrogram slot={SCOPE_SLOT} floorDb={floorDb} />
            </ViewFrame>
          )}
          {shows("vectorscope") && (
            <ViewFrame view="vectorscope">
              <Vectorscope slot={SCOPE_SLOT} />
            </ViewFrame>
          )}
        </div>
      )}
    </div>
  )
}

/**
 * The signal scope: a peak meter, an oscilloscope, a spectrum, a
 * spectrogram and a vectorscope of one source, any of them shown or
 * hidden. Closing it unmounts the body, which lets go of the analyzer.
 */
export function ScopePanel() {
  const open = useScopeStore((state) => state.open)
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeScope()
      }}
    >
      <DialogContent className="max-h-[calc(100%-2rem)] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("Signal scope")}</DialogTitle>
          <DialogDescription>
            {t("Live level, waveform, spectrum and stereo picture of the mix or of one mixer track. Levels are relative to digital full scale.")}
          </DialogDescription>
        </DialogHeader>
        {open && <ScopeBody />}
      </DialogContent>
    </Dialog>
  )
}
