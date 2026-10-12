import { useTranslation } from "@/lib/i18n"

const TOPICS = [
  ["Patterns and songs", "A pattern holds channel notes. Turn on steps in the channel rack or draw notes in the piano roll. Place pattern clips in the playlist, then choose Song in the transport to play the arrangement. Pattern mode repeats only the selected pattern."],
  ["Recording and importing", "For recording, choose an audio input in the recording window, select channels and a destination, then start. Stop and keep the take to create an audio clip. To import, choose an audio file from the channel menu or drag a browser sample into the rack or playlist. Cancelling a picker changes nothing."],
  ["Saving and exporting", "Save stores the editable project and sample references. Export creates an audio file from the selected pattern or whole song. Choose a destination, format and tail before pressing Export. Keep referenced sample files available, or collect them when moving a project."],
  ["Mixer and effects", "Channels feed mixer tracks. Track outputs and sends route audio; detector-only sends feed sidechains. Add effects in the track inspector. Use Undo for musical edits. Display preferences and detached window geometry are saved separately from the project."],
  ["Windows and display", "Use a panel header to detach or dock it. Each window edits the same project. Settings controls appearance, interface scale, language and shortcuts. Supported native plugin editors negotiate content scale; a refusal leaves the editor usable at its existing size."],
  ["Project notebooks", "A notebook has up to eight saved pages. Add a song tick cue and enable Follow song playback to change pages during song playback. Safe HTML preview supports local text formatting and tables. Scripts, embedded objects and remote resources are removed."],
] as const

/** Bundled reference text: no remote request, browser opener or IPC needed. */
export function OfflineManual() {
  const t = useTranslation()
  return <details className="rounded-md border p-3">
    <summary className="cursor-pointer font-medium">{t("Read manual")}</summary>
    <article aria-label={t("Windfall manual")} className="mt-3 flex flex-col gap-3">
      {TOPICS.map(([title, body]) => <section key={title}>
        <h3 className="font-medium">{t(title)}</h3>
        <p className="text-muted-foreground">{t(body)}</p>
      </section>)}
    </article>
  </details>
}
