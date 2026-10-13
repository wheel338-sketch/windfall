import type { PluginBinding } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { useTranslation, message } from "@/lib/i18n"
import { useNativeNoteCapabilities, panLawSupported } from "./note-capabilities"

export function NativeNoteCapabilities({ binding }: { binding: PluginBinding }) {
  const t = useTranslation()
  const { capabilities, error } = useNativeNoteCapabilities(binding.target)
  if (binding.target.type !== "instrument") return null
  const available = t("Available"), unavailable = t("Unavailable")
  return <div className="flex flex-col gap-1">
    <p>{message("Native note support for {name}", { name: binding.name })}</p>
    <p>{message("Pan: {pan}; Volume: {volume}; note instances: {instances}", {
      pan: capabilities?.nativePan[0] ? available : unavailable,
      volume: capabilities?.nativeVolume[0] ? available : unavailable,
      instances: capabilities?.instances ? available : unavailable,
    })}</p>
    {!panLawSupported(capabilities) && <Alert><AlertDescription>{t(capabilities?.projectPanLaw === false ? "Per-note project pan laws are unavailable in this hosting mode" : "This hosted instrument cannot apply the project law to individual notes. Native Pan, Volume and note-instance support are required. Channel and mixer panning still apply.")}</AlertDescription></Alert>}
    {error && <Alert><AlertDescription>{message("Native capability query failed: {error}", { error })}</AlertDescription></Alert>}
  </div>
}
