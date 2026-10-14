import type { PluginBinding } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { useTranslation, message } from "@/lib/i18n"
import { useNativeNoteCapabilities, useNoteChannels, noteRoutes, panLawSupported } from "./note-capabilities"

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
    <p>{message("Pan route: {pan}; Volume route: {volume}", { pan: t(noteRoutes(capabilities, channel).pan === "plugin" ? "In the plugin, per note" : "On the channel, after the plugin"), volume: t(noteRoutes(capabilities, channel).volume === "plugin" ? "In the plugin, per note" : "On the channel, after the plugin") })}</p>
    {capabilities?.projectPanLaw === false && <Alert><AlertDescription>{t("Per-note project pan laws are unavailable in this hosting mode")}</AlertDescription></Alert>}
    {panLawSupported(capabilities, channel) && (noteRoutes(capabilities, channel).pan === "channel" || noteRoutes(capabilities, channel).volume === "channel") && <p>{t("The project pan law applies to every note. A property routed on the channel is applied to the whole output of this instrument after the plugin, and follows the most recent note that sounds; notes that overlap share it.")}</p>}
    </div>)}
    {capabilities?.refusal && <Alert><AlertDescription>{t(capabilities.refusal)}</AlertDescription></Alert>}
    {error && <Alert><AlertDescription>{message("Native capability query failed: {error}", { error })}</AlertDescription></Alert>}
  </div>
}
