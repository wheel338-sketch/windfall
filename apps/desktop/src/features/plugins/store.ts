import { create } from "zustand"
import type { PluginBinding, PluginTarget, RackGraph, RackNode, TrackId } from "@/bindings"
import { registry } from "@/lib/actions/registry"
import { useProjectStore } from "@/lib/store"
import { dynamicTitle } from "@/lib/i18n"

export const usePluginUi = create<{ open: boolean; track?: TrackId }>(() => ({
  open: false,
}))
export function openPluginManager(track?: TrackId) {
  usePluginUi.setState({ open: true, track })
}
export function usePluginBinding(
  target: PluginTarget
): PluginBinding | undefined {
  return useProjectStore((state) => findPluginBinding(state.project, target))
}
export function findPluginBinding(project: import("@/bindings").Project, target: PluginTarget): PluginBinding | undefined {
  if (target.type === "rack") {
    let graph: RackGraph | undefined = project.settings.modularRacks?.find((rack) => rack.channel === target.channel)?.graph
    for (let index = 0; graph && index < target.path.length; index++) {
      const node: RackNode | undefined = graph.nodes.find((node) => node.id === target.path[index])
      if (index === 3 || !target.path[index + 1]) return node?.hosted && sameTarget(node.hosted.target, target) ? node.hosted : undefined
      graph = node?.processor.type === "rack" ? node.processor.graph : undefined
    }
    return undefined
  }
  return project.plugins?.find((plugin) => sameTarget(plugin.target, target))
}
export function sameTarget(a: PluginTarget, b: PluginTarget): boolean {
  if (a.type === "rack") return b.type === "rack" && a.channel === b.channel && a.instrument === b.instrument && a.path.every((id, index) => id === b.path[index])
  return a.type === "instrument"
    ? b.type === "instrument" && a.channel === b.channel
    : a.type === "effect" && b.type === "effect" && a.effect === b.effect
}
export function registerPluginActions() {
  return registry.register([
    {
      id: "plugins.manage",
      ...dynamicTitle("Plugin manager", {}),
      section: "Options",
      keywords: "CLAP scan instruments effects native",
      run: () => openPluginManager(),
    },
  ])
}
