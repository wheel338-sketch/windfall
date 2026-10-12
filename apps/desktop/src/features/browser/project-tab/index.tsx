import { useId, type ReactNode } from "react"

import { Button } from "@/components/ui/button"
import { controlActionLabel } from "@/features/controllers/links-panel"
import { describeTarget } from "@/lib/automation/targets"
import { dispatch } from "@/lib/store/project"
import { selectChannel } from "@/features/channel-rack/channel-ops"
import { navigatePattern, navigateTarget } from "../project-navigation"
import { Badge } from "@/components/ui/badge"
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { effectDescriptor } from "@/features/params/descriptors"
import { useProjectStore } from "@/lib/store/project"

function ProjectGroup({
  title,
  children,
}: {
  title: string
  children: ReactNode
}) {
  const heading = useId()
  return (
    <section aria-labelledby={heading} className="flex flex-col gap-1">
      <h2 id={heading} className="text-xs font-semibold text-muted-foreground">
        {title}
      </h2>
      {children}
    </section>
  )
}

function NamedGroup({
  title,
  entries,
  always = false,
  onPick,
}: {
  title: string
  entries: readonly { name: string }[]
  always?: boolean
  onPick?: (index: number) => void
}) {
  if (!always && entries.length === 0) return null
  return (
    <ProjectGroup title={title}>
      {entries.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No patterns yet</EmptyTitle>
          </EmptyHeader>
        </Empty>
      ) : (
        <ul className="flex flex-col gap-1">
          {entries.map((entry, index) => (
            <li key={index} className="rounded-sm px-2 py-1 break-words">
              {onPick ? <Button variant="ghost" size="sm" onClick={() => onPick(index)}>{entry.name}</Button> : entry.name}
            </li>
          ))}
        </ul>
      )}
    </ProjectGroup>
  )
}

/** A live inventory with editor navigation and saved controller-link removal. */
export function ProjectTab() {
  const project = useProjectStore((state) => state.project)
  const history = useProjectStore((state) => state.history)
  const effectTracks = project.mixer.tracks.filter(
    (track) => track.effects.length > 0
  )

  return (
    <div data-slot="project-browser" className="flex flex-col gap-4 p-2">
      <p className="text-xs text-muted-foreground">Open project</p>
      <NamedGroup title="Patterns" entries={project.patterns} always onPick={(index) => void navigatePattern(project.patterns[index].id)} />
      <NamedGroup title="Channels" entries={project.channels} onPick={(index) => selectChannel(project.channels[index].id, { openSettings: true })} />
      <NamedGroup title="Samples" entries={project.samples} />
      {effectTracks.length > 0 && (
        <ProjectGroup title="Mixer effects">
          {effectTracks.map((track) => (
            <section key={track.id} aria-label={track.name} className="px-2">
              <h3 className="text-xs font-medium break-words">{track.name}</h3>
              <ul className="flex flex-col gap-1">
                {track.effects.map((slot) => {
                  const plugin = project.plugins?.find(
                    (binding) =>
                      binding.target.type === "effect" &&
                      binding.target.effect === slot.id
                  )
                  return (
                    <li
                      key={slot.id}
                      className="flex items-center gap-2 py-1 pl-2"
                    >
                      <span className="min-w-0 break-words">
                        {plugin?.name ??
                          effectDescriptor(slot.params.type).name}
                      </span>
                      {!slot.enabled && (
                        <Badge variant="outline">Bypassed</Badge>
                      )}
                    </li>
                  )
                })}
              </ul>
            </section>
          ))}
        </ProjectGroup>
      )}
      <NamedGroup title="Automations" entries={project.automations} onPick={(index) => navigateTarget(project.automations[index].target)} />
      {(project.settings.controllerLinks?.length ?? 0) > 0 && <ProjectGroup title="Controller links">
        <ul className="flex flex-col gap-2">{project.settings.controllerLinks?.map((link, index) => <li key={index} className="flex flex-wrap items-center gap-2">
          <span>{link.source.type === "midi" ? `${link.source.device ?? "Any MIDI input"} · ${link.source.control.type}` : `Internal ${link.source.controller}:${link.source.output}`}</span>
          {link.target.type === "param" ? <Button variant="ghost" size="sm" onClick={() => { if (link.target.type === "param") navigateTarget(link.target.target) }}>{describeTarget(project, link.target.target)}</Button> : <span>{link.target.type === "action" ? controlActionLabel(link.target.action) : link.target.slot.type}</span>}
          <Button variant="outline" size="sm" aria-label={`Remove controller link ${index + 1}`} onClick={() => { if (useProjectStore.getState().project !== project) return; void dispatch({ type: "setControllerLinks", links: project.settings.controllerLinks?.filter((_, at) => at !== index) ?? [] }) }}>Remove</Button>
        </li>)}</ul>
      </ProjectGroup>}
      <NamedGroup title="Plugins" entries={project.plugins ?? []} />
      <NamedGroup
        title="Retained plugin states"
        entries={(project.retainedPlugins ?? []).map((plugin) => ({
          name: plugin.name ?? plugin.internalName,
        }))}
      />
      {history.entries.length > 0 && (
        <ProjectGroup title="Edit history">
          <ol className="flex flex-col gap-1">
            {history.entries.map((entry, index) => {
              const undone = index >= history.cursor
              return (
                <li
                  key={index}
                  data-undone={undone}
                  className="flex items-center gap-2 rounded-sm px-2 py-1 data-[undone=true]:text-muted-foreground"
                >
                  <span className="min-w-0 break-words">{entry.label}</span>
                  <Badge variant={undone ? "outline" : "secondary"}>
                    {undone ? "Undone" : "Applied"}
                  </Badge>
                </li>
              )
            })}
          </ol>
        </ProjectGroup>
      )}
    </div>
  )
}
