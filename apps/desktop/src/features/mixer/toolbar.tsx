import type { MixerDock, MixerTrackPatch } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { dispatch, useProjectStore, useUiStore } from "@/lib/store"
import { MIXER_LAYOUTS } from "./layout"
import { useMixerUi, selectedMixerTracks } from "./mixer-ui"
import { outputChoices } from "./routing"
import { useWaveformUi } from "./waveform-meter"
import { openMixerRender, renderableMixerTracks } from "@/features/export/mixer-render"
import { translatedText as t, message } from "@/lib/i18n"

export function MixerToolbar({ filter, onFilterChange }: {
  filter: string
  onFilterChange: (query: string) => void
}) {
  const tracks = useProjectStore((state) => state.project.mixer.tracks)
  const selection = useMixerUi((state) => state.selected)
  const primary = useUiStore((state) => state.selectedTrack)
  const layout = useUiStore((state) => state.mixerLayout)
  const meterMode = useUiStore((state) => state.mixerMeterMode)
  const waveformError = useWaveformUi((state) => state.error)
  const selected = tracks.filter((track) => (selection.length ? selection : [primary]).includes(track.id))
  const inserts = selected.filter((track) => track.id !== 0 && !track.current)
  const sourceIds = new Set(inserts.map((source) => source.id))
  const allowedTargets = inserts.map((source) => new Set(outputChoices(tracks, source.id).tracks.map((candidate) => candidate.id)))
  const routeChoices = tracks.filter((track) => inserts.length > 0 && !sourceIds.has(track.id) && allowedTargets.every((targets) => targets.has(track.id)))
  const moveChoices = tracks.filter((track) => track.id !== 0 && !track.current && !inserts.some((source) => source.id === track.id))
  function patch(patch: MixerTrackPatch, ordinary = false) {
    const targets = selectedMixerTracks().filter((track) => !ordinary || track.id !== 0 && !track.current)
    if (targets.length) void dispatch({ type: "batch", commands: targets.map((track) => ({ type: "updateMixerTrack", id: track.id, patch })) })
  }
  function selectAll() {
    const ids = tracks.filter((track) => track.id !== 0 && !track.current).map((track) => track.id)
    useMixerUi.setState({ selected: ids, anchor: ids[0] ?? null })
    useUiStore.getState().selectTrack(ids[0] ?? null)
  }
  return <div className="absolute inset-x-0 top-0 z-20 flex h-9 items-center gap-2 border-b bg-chassis px-2">
    <Input
      aria-label={t("Filter tracks")}
      placeholder={t("Filter tracks")}
      className="w-40 shrink-0"
      value={filter}
      onChange={(event) => onFilterChange(event.target.value)}
      onKeyDownCapture={(event) => {
        if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
          event.stopPropagation()
        }
      }}
    />
    <Select items={[...MIXER_LAYOUTS]} value={layout} onValueChange={(value: string | null) => { if (value) useUiStore.getState().setMixerLayout(value) }}>
      <SelectTrigger size="sm" className="w-32" aria-label={t("Mixer size layout")}><SelectValue /></SelectTrigger>
      <SelectContent><SelectGroup>{MIXER_LAYOUTS.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
    </Select>
    <Button size="sm" variant="outline" aria-pressed={meterMode === "waveform"} title={waveformError || t("Switch between peak levels and stereo waveform history")} onClick={() => useUiStore.getState().setMixerMeterMode(meterMode === "level" ? "waveform" : "level")}>{meterMode === "waveform" ? t("Waveforms") : t("Levels")}</Button>
    {waveformError && <span role="status" className="max-w-40 truncate text-xs text-destructive" title={waveformError}>{t("Waveform feed unavailable")}</span>}
    <Button size="sm" variant="ghost" onClick={selectAll}>{t("Select inserts")}</Button>
    <Button size="sm" variant="ghost" disabled={!selected.length} onClick={() => { useUiStore.getState().selectTrack(null); useMixerUi.setState({ selected: [], anchor: null }) }}>{t("Clear")}</Button>
    <Button size="sm" variant="outline" disabled={!renderableMixerTracks(selected).length} onClick={() => openMixerRender(selected)}>{t("Render selected…")}</Button>
    <Button size="sm" variant="outline" disabled={!tracks.some((track) => track.recording?.armed && !track.current)} onClick={() => openMixerRender(tracks.filter((track) => track.recording?.armed))}>{t("Render armed…")}</Button>
    <Popover><PopoverTrigger render={<Button size="sm" variant="outline" disabled={!selected.length}>{selected.length}{" "}{t("selected")}</Button>} />
      <PopoverContent align="start" className="w-80 space-y-3">
        <p className="text-xs text-muted-foreground">{t("Ctrl-click toggles tracks. Shift-click selects a range. Faders change together by the same gain ratio; pans move by the same amount.")}</p>
        <div className="flex flex-wrap gap-1">
          <Button size="sm" variant="outline" onClick={() => patch({ muted: !selected.every((track) => track.muted) })}>{t("Mute / unmute")}</Button>
          <Button size="sm" variant="outline" disabled={!inserts.length} onClick={() => patch({ solo: !inserts.every((track) => track.solo) }, true)}>{t("Solo / unsolo")}</Button>
          <Button size="sm" variant="outline" onClick={() => patch({ volume: 1 })}>0 dB</Button>
          <Button size="sm" variant="outline" onClick={() => patch({ pan: 0 })}>{t("Center pan")}</Button>
        </div>
        <div className="flex gap-1">{(["left", "middle", "right"] as MixerDock[]).map((dock) => <Button key={dock} size="sm" variant="outline" disabled={!inserts.length} onClick={() => patch({ dock }, true)}>{t("Dock")}{" "}{t(dock)}</Button>)}</div>
        <Select value={null} disabled={!inserts.length} items={[{ value: "none", label: t("None (sends only)") }, ...routeChoices.map((track) => ({ value: String(track.id), label: track.name }))]} onValueChange={(value: string | null) => {
          if (value !== null) void dispatch({ type: "batch", commands: inserts.map((track) => ({ type: "setTrackOutput", id: track.id, output: value === "none" ? undefined : Number(value) })) })
        }}><SelectTrigger aria-label={t("Route selected tracks")}><SelectValue placeholder={t("Route selected tracks to…")} /></SelectTrigger><SelectContent><SelectGroup>
          {routeChoices.map((track) => <SelectItem key={track.id} value={String(track.id)}>{track.name}</SelectItem>)}<SelectItem value="none">{t("None (sends only)")}</SelectItem>
        </SelectGroup></SelectContent></Select>
        <Select value={null} disabled={!inserts.length} items={[...moveChoices.map((track) => ({ value: String(track.id), label: message("Before {trackName}", { trackName: track.name }) })), { value: "end", label: t("End of mixer") }]} onValueChange={(value: string | null) => {
          if (value !== null) void dispatch({ type: "moveMixerTracks", expected: tracks.map((track) => track.id), ids: inserts.map((track) => track.id), before: value === "end" ? null : Number(value) })
        }}><SelectTrigger aria-label={t("Move selected tracks")}><SelectValue placeholder={t("Move selected tracks…")} /></SelectTrigger><SelectContent><SelectGroup>
          {moveChoices.map((track) => <SelectItem key={track.id} value={String(track.id)}>{t("Before")}{" "}{track.name}</SelectItem>)}<SelectItem value="end">{t("End of mixer")}</SelectItem>
        </SelectGroup></SelectContent></Select>
      </PopoverContent>
    </Popover>
  </div>
}
