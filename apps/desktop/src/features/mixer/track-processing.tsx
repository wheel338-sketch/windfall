import { useMemo, useState } from "react"
import type { AutomationTarget, TrackId } from "@/bindings"
import { Button } from "@/components/ui/button"
import { useAutomation } from "@/features/automation/live"
import { ParamControl, clampParam, readParam } from "@/features/params"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { DEFAULT_TRACK_PROCESSING, TRACK_PROCESSING_INFO } from "@/lib/track-processing"
import { useGestureValue } from "./use-gesture-value"
import { translatedText as t } from "@/lib/i18n"

function Control({ track, param, label }: { track: TrackId; param: number; label?: string }) {
  const info = TRACK_PROCESSING_INFO[param]
  const value = useProjectStore((state) => readParam(state.project.mixer.tracks.find((item) => item.id === track)?.processing ?? DEFAULT_TRACK_PROCESSING, info))
  const gesture = useGestureValue(value, (value) => ({ type: "setTrackParam", id: track, param, value }), (value) => clampParam(info, value))
  const target = useMemo<AutomationTarget>(() => ({ type: "trackParam", track, param }), [track, param])
  const automation = useAutomation(target)
  return <ParamControl info={info} size="sm" label={label} contextItems={automation.items} live={automation.live} marker={automation.marker} {...gesture} />
}

export function TrackProcessingPanel({ track }: { track: TrackId }) {
  const [open, setOpen] = useState(false)
  return <section className="shrink-0 border-b p-1.5" aria-label={t("Integrated track EQ and stereo utilities")}>
    <div className="flex items-center justify-between gap-2"><Button size="sm" variant="outline" aria-expanded={open} className="flex-1 justify-start" onClick={() => setOpen(!open)}>{t("Track EQ and stereo")}</Button>{open && <Button size="sm" variant="ghost" onClick={() => void dispatch({ type: "updateMixerTrack", id: track, patch: { processing: DEFAULT_TRACK_PROCESSING } })}>{t("Reset")}</Button>}</div>
    {open && <div className="space-y-3 pt-2">
      <p className="text-xs text-muted-foreground">{t("After the effect slots, before the fader. Right-click a control to automate it.")}</p>
      <Control track={track} param={0} />
      {([[1, "Low shelf"], [5, "Mid bell"], [9, "High shelf"]] as const).map(([start, title]) => {
        const index = Number(start)
        return <fieldset key={index} className="rounded-md border px-2 py-1"><legend className="px-1 text-xs">{t(title)}</legend><div className="grid grid-cols-4 items-end gap-2"><Control track={track} param={index} label={t("On")} /><Control track={track} param={index + 1} label="Hz" /><Control track={track} param={index + 2} label="dB" /><Control track={track} param={index + 3} label="Q" /></div></fieldset>
      })}
      <div className="grid grid-cols-4 items-end gap-2"><Control track={track} param={13} label={t("Invert L")} /><Control track={track} param={14} label={t("Invert R")} /><Control track={track} param={15} label={t("Swap")} /><Control track={track} param={16} label={t("Stereo")} /></div>
      <p className="text-xs text-muted-foreground">{t("Stereo separation: −1 doubles the side signal, 0 preserves stereo, +1 folds to mono.")}</p>
    </div>}
  </section>
}
