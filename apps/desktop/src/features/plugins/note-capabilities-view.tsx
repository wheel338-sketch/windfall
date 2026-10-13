import type { PluginBinding } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { useTranslation, message } from "@/lib/i18n"
import { useNativeNoteCapabilities, useNoteChannels, panLawSupported } from "./note-capabilities"

export function NativeNoteCapabilities({ binding }: { binding: PluginBinding }) {
  const t = useTranslation()
  const { capabilities, changed, error } = useNativeNoteCapabilities(binding.target)
  const channels = useNoteChannels(binding.target, capabilities)
  if (binding.target.type !== "instrument") return null
  const available = t("Available"), unavailable = t("Unavailable")
  return <div className="flex flex-col gap-1">
    <p>{message("Native note support for {name}", { name: binding.name })}</p>
    {changed && <p role="status">{t("Plugin capabilities changed")}</p>}
    {channels.map((channel) => <div key={channel} className="flex flex-col gap-1">
    <p>{message("Note MIDI channel {channel}", { channel: channel + 1 })}</p>
    <p>{message("Pan: {pan}; Volume: {volume}; note instances: {instances}", {
      pan: capabilities?.nativePan[channel] ? available : unavailable,
      volume: capabilities?.nativeVolume[channel] ? available : unavailable,
      instances: capabilities?.instances ? available : unavailable,
    })}</p>
    <p>{message("Pan route: {pan}; Volume route: {volume}", { pan: t(capabilities?.instances && capabilities.nativePan[channel] ? "Native" : "Unavailable"), volume: t(capabilities?.instances && capabilities.nativeVolume[channel] ? "Native" : "Unavailable") })}</p>
    {!panLawSupported(capabilities, channel) && <Alert><AlertDescription>{t(capabilities?.projectPanLaw === false ? "Per-note project pan laws are unavailable in this hosting mode" : "This hosted instrument cannot apply the project law to individual notes. Native Pan, Volume and note-instance support are required. Channel and mixer panning still apply.")}</AlertDescription></Alert>}
    </div>)}
    {capabilities?.refusal && <Alert><AlertDescription>{t(capabilities.refusal)}</AlertDescription></Alert>}
    {error && <Alert><AlertDescription>{message("Native capability query failed: {error}", { error })}</AlertDescription></Alert>}
  </div>
}
