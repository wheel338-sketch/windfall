import { useTranslation } from "@/lib/i18n"
import { useUiStore, activeScopeOf, type PanelId } from "@/lib/store/ui"
const HELP: Record<PanelId, string> = {
  browser: "Preview sounds in the browser, then drag a sound to the rack or playlist. Your sample files stay unchanged.",
  channelRack: "Turn on steps to make a beat. The selected pattern and piano roll share the same notes. Use channel settings to choose a sound and mixer destination.",
  pianoRoll: "Draw and edit the selected channel's notes. Choose a tool and snap before dragging. Undo reverses ordinary note edits.",
  playlist: "Place patterns, audio and automation on song lanes. Switch the transport to Song to hear the arrangement; Pattern plays only the selected pattern.",
  mixer: "Each channel feeds a mixer track. Add effects, adjust levels and route sends here. The master sums the song; stop playback before changing audio devices.",
}
export function ContextHelp({ panel }: { panel?: PanelId }) {
  const t = useTranslation()
  const scope = useUiStore(activeScopeOf)
  const actual = panel ?? (scope === "rackInspector" ? "channelRack" : scope === "clipInspector" ? "playlist" : scope === "effect" || scope === "effectInspector" ? "mixer" : scope)
  return <section aria-label={t("Contextual help")} className="flex flex-col gap-1">
    <h3 className="font-medium">{t("Contextual help")}</h3>
    <p className="text-muted-foreground">{t(HELP[actual])}</p>
  </section>
}
