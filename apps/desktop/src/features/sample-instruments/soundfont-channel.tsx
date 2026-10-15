import { useEffect, useState } from "react"
import { create } from "zustand"

import type { SoundFontPreset } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { FieldDescription } from "@/components/ui/field"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { message, translatedText as t } from "@/lib/i18n"

import { SoundFontPresetList } from "./editor"

/**
 * The dialog that adds a channel playing one preset of a SoundFont: from a
 * `.sf2` file in the browser, or, with `generalMidi`, from the General MIDI
 * SoundFont chosen in Settings.
 */
type SoundFontChannelRequest = { path: string | null; generalMidi: boolean }

export const useSoundFontChannelDialog = create<{
  request: SoundFontChannelRequest | null
  open(request: SoundFontChannelRequest): void
  close(): void
}>((set) => ({
  request: null,
  open: (request) => set({ request }),
  close: () => set({ request: null }),
}))

/** Asks which preset of `path` the new channel plays. */
export function addChannelFromSoundFont(path: string) {
  useSoundFontChannelDialog.getState().open({ path, generalMidi: false })
}

/** Asks which General MIDI instrument the new channel plays, from the SoundFont chosen in Settings. */
export async function addGeneralMidiChannel() {
  let path: string | null
  try { path = (await backend.defaultSoundfont?.()) ?? null } catch { path = null }
  useSoundFontChannelDialog.getState().open({ path, generalMidi: true })
}

export function SoundFontChannelDialogHost() {
  const request = useSoundFontChannelDialog((state) => state.request)
  const close = useSoundFontChannelDialog((state) => state.close)
  const [presets, setPresets] = useState<SoundFontPreset[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const path = request?.path ?? null
  useEffect(() => {
    if (!path || !backend.soundfontPresets) return
    let current = true
    void backend.soundfontPresets(path)
      .then((found) => { if (current) { setPresets(found); setError(null) } })
      .catch((failure: unknown) => { if (current) { setPresets([]); setError(errorMessage(failure)) } })
    return () => { current = false }
  }, [path])
  const name = path ? path.split(/[\\/]/).pop() ?? path : ""
  return <Dialog open={request !== null} onOpenChange={(next) => { if (!next) { close(); setPresets([]); setError(null) } }}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-md">
      <DialogTitle>{request?.generalMidi ? t("Add a General MIDI instrument") : t("Add a channel from a SoundFont")}</DialogTitle>
      <DialogDescription>
        {path ? message("Choose a preset of {file}. The new channel plays it, as one undo step.", { file: name }) : t("Choose a General MIDI SoundFont in Settings first. Windfall does not include one.")}
      </DialogDescription>
      {!path && request?.generalMidi && <Button variant="outline" onClick={() => { close(); useUiStore.getState().openDialog("settings") }}>{t("Open Settings")}</Button>}
      {path && presets.length > 0 && <SoundFontPresetList presets={presets} disabled={busy || !backend.soundfontAddChannel}
        choose={async (preset) => {
          const generation = getProjectGeneration()
          setBusy(true)
          try {
            const result = await backend.soundfontAddChannel!(path, preset.bank, preset.program, null)
            if (generation === getProjectGeneration()) { receivePatch(result.patch); close(); setPresets([]); setError(null) }
          } catch (failure) { setError(errorMessage(failure)) }
          finally { setBusy(false) }
        }} />}
      {!backend.soundfontAddChannel && <FieldDescription>{t("SoundFont file loading requires the desktop app.")}</FieldDescription>}
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    </DialogContent>
  </Dialog>
}

/** Settings: the General MIDI SoundFont that General MIDI instruments play from. */
export function GeneralMidiSoundFontSection() {
  const [path, setPath] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let current = true
    void backend.defaultSoundfont?.().then((found) => { if (current) setPath(found) })
      .catch((failure: unknown) => { if (current) setError(errorMessage(failure)) })
    return () => { current = false }
  }, [])
  if (!backend.setDefaultSoundfont) return null
  const choose = async (next: string | null) => {
    try { setPath(await backend.setDefaultSoundfont!(next)); setError(null) }
    catch (failure) { setError(errorMessage(failure)) }
  }
  return <section className="flex flex-col gap-2" aria-label={t("General MIDI SoundFont")}>
    <h3 className="text-xs font-medium">{t("General MIDI SoundFont")}</h3>
    <p data-default-soundfont className="truncate text-sm" title={path ?? undefined}>{path ?? t("None chosen")}</p>
    <div className="flex gap-1">
      <Button variant="outline" size="sm" onClick={async () => {
        try { const picked = await backend.pickAudioFile("soundFont"); if (picked) await choose(picked) }
        catch (failure) { setError(errorMessage(failure)) }
      }}>{t("Choose file…")}</Button>
      {path && <Button variant="ghost" size="sm" onClick={() => void choose(null)}>{t("Forget it")}</Button>}
    </div>
    <FieldDescription>{t("Add channel offers General MIDI instruments by family from this SoundFont. Windfall does not include one; use your own General MIDI bank.")}</FieldDescription>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </section>
}
