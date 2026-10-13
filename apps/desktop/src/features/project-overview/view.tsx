import { Button } from "@/components/ui/button"
import { navigateClip, navigatePattern } from "@/features/browser/project-navigation"
import { selectChannel } from "@/features/channel-rack/channel-ops"
import { closeProjectOverview } from "./actions"
import type { ClipContent } from "@/bindings"
import { useProjectStore } from "@/lib/store/project"
import { translatedText as t, message } from "@/lib/i18n"

type TargetNames = {
  patterns: Map<number, string>
  samples: Map<number, string>
  automations: Map<number, string>
}

function clipTargetName(content: ClipContent, names: TargetNames): string {
  switch (content.type) {
    case "pattern":
      return message("Pattern: {get}", { get: names.patterns.get(content.pattern) ?? t("Missing pattern target") })
    case "audio":
      return message("Sample: {get}", { get: names.samples.get(content.sample) ?? t("Missing sample target") })
    case "automation":
      return message("Automation: {get}", { get: names.automations.get(content.automation) ?? t("Missing automation target") })
  }
}

/** Select a document relationship and navigate to its editor. */
export function ProjectOverviewView() {
  const project = useProjectStore((state) => state.project)
  const names: TargetNames = {
    patterns: new Map(project.patterns.map(({ id, name }) => [id, name])),
    samples: new Map(project.samples.map(({ id, name }) => [id, name])),
    automations: new Map(project.automations.map(({ id, name }) => [id, name])),
  }
  const tracks = new Map(
    project.playlist.tracks.map(({ id, name }) => [id, name])
  )
  const patterns = project.patterns.map((pattern) => ({
    ...pattern,
    activeChannels: new Set(
      pattern.lanes
        .filter((lane) => lane.notes.length > 0)
        .map((lane) => lane.channel)
    ),
  }))
  const clips = project.playlist.clips.toSorted(
    (a, b) => a.start - b.start || a.id - b.id
  )

  return (
    <div className="flex min-h-0 flex-1 min-w-0 flex-col gap-4 overflow-y-auto">
      <section aria-label={t("Channel and pattern relationships")}>
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-xs">
            <caption className="mb-2 text-left font-medium">
              {t("Channels and patterns")}
            </caption>
            <thead>
              <tr>
                <th scope="col" className="border p-2 text-left">
                  {t("Channel")}
                </th>
                {patterns.map((pattern) => (
                  <th
                    key={pattern.id}
                    scope="col"
                    className="min-w-24 border p-2 text-center"
                  >
                    <Button variant="ghost" size="sm" onClick={() => { void navigatePattern(pattern.id); closeProjectOverview() }}>{pattern.name}</Button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {project.channels.map((channel) => (
                <tr key={channel.id}>
                  <th scope="row" className="border p-2 text-left font-medium">
                    <Button variant="ghost" size="sm" onClick={() => { selectChannel(channel.id, { openSettings: true }); closeProjectOverview() }}>{channel.name}</Button>
                  </th>
                  {patterns.map((pattern) => {
                    const marked = pattern.activeChannels.has(channel.id)
                    return (
                      <td
                        key={pattern.id}
                        aria-label={marked ? t("Has notes") : t("No notes")}
                        className="border p-2 text-center"
                      >
                        <Button variant="ghost" size="sm" aria-label={message("Open {patternName} / {channelName}{marked}", { patternName: pattern.name, channelName: channel.name, marked: marked ? t(" with notes") : t(" empty lane") })} onClick={() => { void navigatePattern(pattern.id, channel.id); closeProjectOverview() }}><span aria-hidden="true">{marked ? "●" : "—"}</span></Button>
                      </td>
                    )
                  })}
                </tr>
              ))}
              {project.channels.length === 0 && (
                <tr>
                  <td colSpan={patterns.length + 1} className="p-2">
                    {t("No channels in this project.")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>
      <section aria-label={t("Playlist clips")} className="flex flex-col gap-2">
        <h2 className="font-medium">{t("Playlist clips")}</h2>
        <ol
          aria-label={t("Playlist clips in timeline order")}
          className="flex flex-col gap-1"
        >
          {clips.map((clip) => (
            <li
              key={clip.id}
              className="flex flex-wrap gap-x-3 gap-y-1 rounded border p-2"
            >
              <span>{tracks.get(clip.track) ?? t("Missing playlist track")}</span>
              <Button variant="ghost" size="sm" onClick={() => { navigateClip(clip.id); closeProjectOverview() }}>{clipTargetName(clip.content, names)}</Button>
              {clip.content.type === "pattern" && project.patterns.find((p) => p.id === (clip.content.type === "pattern" ? clip.content.pattern : -1))?.lanes.filter((lane) => lane.notes.length > 0).map((lane) => <Button variant="outline" size="sm" key={lane.channel} onClick={() => { if (clip.content.type === "pattern") void navigatePattern(clip.content.pattern, lane.channel); closeProjectOverview() }}>{project.channels.find((c) => c.id === lane.channel)?.name ?? t("Missing channel")}</Button>)}
              <span className="text-muted-foreground">
                {t("Start:")}{" "}{clip.start}{" "}{t("ticks")}
              </span>
            </li>
          ))}
          {clips.length === 0 && <li>{t("No playlist clips in this project.")}</li>}
        </ol>
      </section>
    </div>
  )
}
