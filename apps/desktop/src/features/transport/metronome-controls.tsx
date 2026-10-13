import { ActionButton } from "@/components/action-button"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { DEFAULT_METRONOME, setMetronome, useTransportStore } from "@/lib/store/transport"
import { METRONOME_GAINS, nextMetronomeGain } from "./metronome-gain"
import { nextMetronomeGainScale } from "./metronome-gain-scale"
import { nextMetronomePreset } from "./metronome-preset-step"
import { useRecordingStore } from "./recording-store"
import { translatedText as t, message } from "@/lib/i18n"

export function MetronomeControls() {
  const settings = useTransportStore((state) => state.metronome ?? DEFAULT_METRONOME)
  const recording = useRecordingStore((state) => state.state.active)
  return <div className="flex items-center gap-0.5">
    <ActionButton action="transport.metronome" variant={settings.enabled ? "secondary" : "ghost"} size="sm" aria-pressed={settings.enabled}>{t("Click")}</ActionButton>
    <Popover><PopoverTrigger render={<Button variant="ghost" size="sm" aria-label={t("Metronome settings")}>⋯</Button>} /><PopoverContent className="w-64 space-y-3">
      <p className="text-sm font-medium">{t("Metronome")}</p>
      <label className="block space-y-1 text-xs">{t("Click volume (%)")}<Input type="number" min={0} max={100} value={Math.round(settings.gain * 100)} disabled={recording} onChange={(event) => { const value = Number(event.target.value); if (event.target.value && Number.isFinite(value) && value >= 0 && value <= 100) void setMetronome({ gain: value / 100 }) }} /></label>
      <div className="flex gap-1">
        {METRONOME_GAINS.map((preset) => (
          <Button key={preset.label} variant="outline" size="sm" disabled={recording || nextMetronomeGain(settings.gain, preset.value) === null} onClick={() => {
            const value = nextMetronomeGain(settings.gain, preset.value)
            if (value !== null) void setMetronome({ gain: value })
          }}>{preset.label}</Button>
        ))}
      </div>
      <div className="flex gap-1">
        {(["previous", "next"] as const).map((direction) => (
          <Button key={direction} variant="outline" size="sm" aria-label={message("Choose the {direction} metronome volume", { direction })} disabled={recording || nextMetronomePreset(settings.gain, direction) === null} onClick={() => {
            if (useRecordingStore.getState().state.active) return
            const latest = useTransportStore.getState().metronome?.gain ?? DEFAULT_METRONOME.gain
            const next = nextMetronomePreset(latest, direction)
            if (next !== null) void setMetronome({ gain: next })
          }}>{direction === "previous" ? t("Previous") : t("Next")}</Button>
        ))}
      </div>
      <div className="flex gap-1">
        {(["half", "double"] as const).map((factor) => (
          <Button key={factor} variant="outline" size="sm" disabled={recording || nextMetronomeGainScale(settings.gain, factor) === null} onClick={() => {
            if (useRecordingStore.getState().state.active) return
            const gain = useTransportStore.getState().metronome?.gain ?? DEFAULT_METRONOME.gain
            const next = nextMetronomeGainScale(gain, factor)
            if (next !== null) void setMetronome({ gain: next })
          }}>{factor === "half" ? t("Half") : t("Double")}</Button>
        ))}
      </div>
      <Button variant={settings.accent ? "secondary" : "outline"} size="sm" aria-pressed={settings.accent} disabled={recording} onClick={() => void setMetronome({ accent: !settings.accent })}>{t("Accent the first beat")}</Button>
    </PopoverContent></Popover>
  </div>
}
