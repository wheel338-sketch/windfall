import { toast } from "sonner"
import { addInternalLink, internalSources } from "@/features/internal-controllers/model"

import type { AutomationTarget, ControllerLink, LinkTarget } from "@/bindings"
import type { ContextItem } from "@/components/context-actions"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store/project"

/** Whether two plain values hold the same fields, in whatever order. */
export function sameShape(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (
    typeof a !== "object" ||
    typeof b !== "object" ||
    a === null ||
    b === null
  )
    return false
  const left = Object.entries(a)
  const right = new Map(Object.entries(b))
  return (
    left.length === right.size &&
    left.every(([key, value]) => right.has(key) && sameShape(value, right.get(key)))
  )
}

/** The links of the project that move `target`. */
export function linksOf(
  links: readonly ControllerLink[],
  target: AutomationTarget
): ControllerLink[] {
  return links.filter(
    (link) =>
      link.target.type === "param" && sameShape(link.target.target, target)
  )
}

/**
 * Starts a pass of linking: the next hardware control that moves is linked
 * to each target in turn. The shell makes the links; they arrive as project
 * patches like any other edit.
 */
export async function learnTargets(
  targets: LinkTarget[],
  multi: boolean
): Promise<void> {
  try {
    const state = await backend.controllerState()
    await backend.controllerLearn(targets, false, multi, state.generation)
    toast(
      multi
        ? "Multi-link is on"
        : "Move a hardware control to link it",
      {
        description: multi
          ? "Move controls on screen, then move the hardware controls in the same order."
          : "The next knob, fader or key that sends is linked.",
      }
    )
  } catch (error) {
    reportError(error, "Could not start linking")
  }
}

/**
 * What the menu of a control offers about hardware controllers: link it,
 * and remove the links it has. Nothing in a plain browser, where there is
 * no MIDI hardware.
 */
export function controllerEntries(target: AutomationTarget): ContextItem[] {
  const links = useProjectStore.getState().project.settings.controllerLinks ?? []
  const linked = linksOf(links, target)
  const items: ContextItem[] = [{
    dynamic: () => {
      const sources = internalSources(useProjectStore.getState().project)
      return [{ submenu: "Link to internal controller", items: sources.length ? sources.map((source) => ({ submenu: source.name, items: source.ports.map((port, output) => ({ title: port, disabled: (useProjectStore.getState().project.settings.controllerLinks ?? []).some((link) => link.source.type === "internal" && link.source.controller === source.id && link.source.output === output && link.target.type === "param" && sameShape(link.target.target, target)), run: () => addInternalLink(source.id, output, target) })) })) : [{ title: "Add a controller in a mixer effect slot or add a Dashboard channel", disabled: true, run: () => {} }] }]
    },
  }]
  if (backend.kind === "tauri") items.push(
    {
      title: linked.length > 0 ? "Link to another controller…" : "Link to controller…",
      run: () => {
        void learnTargets([{ type: "param", target }], false)
      },
    },
  )
  if (linked.length > 0) {
    items.push({
      title: "Remove controller link",
      destructive: true,
      run: () => {
        void dispatch({
          type: "setControllerLinks",
          links: links.filter((link) => !linked.includes(link)),
        })
      },
    })
  }
  return items
}
