import { formatNumber, message as i18nMessage, dynamicTitle } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { create } from "zustand"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { registry } from "@/lib/actions"
import { useProjectStore } from "@/lib/store/project"
import { pluginDropHandlers } from "./insert"
import { PluginCatalog } from "./catalog"
export const usePluginPicker = create(() => ({ open: false }))
export function PluginPicker() {
  useLanguageStore((state) => state.language)
  const project = useProjectStore((s) => s.project)
  const open = usePluginPicker((s) => s.open)
  return <Dialog open={open} onOpenChange={(open) => usePluginPicker.setState({ open })}><DialogContent className="flex h-dvh max-w-none! flex-col rounded-none"><DialogHeader><DialogTitle>{t("Plugin picker")}</DialogTitle><DialogDescription>{t("Search or filter favorites. Double-click a tile to add it. Drag instruments to a channel or effects to a mixer slot.")}</DialogDescription></DialogHeader>{open && <><div className="min-h-0 flex-1 overflow-auto"><PluginCatalog tiles /></div>
    <section aria-label={t("Plugin drop destinations")} className="shrink-0 border-t pt-2">
      <p className="text-sm text-muted-foreground">{t("Drop an instrument on a channel, or an effect on a mixer slot.")}</p>
      <div className="flex max-h-36 flex-wrap gap-2 overflow-auto">
        {project.channels.map((channel) => <button type="button" key={`channel-${channel.id}`} {...pluginDropHandlers({ channel: channel.id })} aria-label={i18nMessage("Drop instrument on {value0}", { value0: channel.name })} className="rounded border px-3 py-2 text-sm">{channel.name}</button>)}
        {project.mixer.tracks.map((track) => <div key={`track-${track.id}`} className="flex flex-wrap gap-1 rounded border p-1">
          {track.effects.map((effect,index) => <button type="button" key={effect.id} {...pluginDropHandlers({ track: track.id, effect: effect.id })} aria-label={i18nMessage("Drop effect on {value0} slot {value1}", { value0: track.name, value1: index+1 })} className="rounded border px-2 py-1 text-sm">{track.name} · {formatNumber(index+1)}</button>)}
          <button type="button" {...pluginDropHandlers({ track: track.id })} aria-label={i18nMessage("Drop effect on {value0} new slot", { value0: track.name })} className="rounded border px-2 py-1 text-sm">{i18nMessage("{track} · add effect", { track: track.name })}</button>
        </div>)}
      </div>
    </section></>}</DialogContent></Dialog>
}
export function registerPluginPickerActions() { return registry.register([{ id: "plugins.picker", ...dynamicTitle("Plugin picker", {}), section: "View", keywords: "plugin tiles grid favorites thumbnails", run: () => usePluginPicker.setState({ open: true }) }]) }
