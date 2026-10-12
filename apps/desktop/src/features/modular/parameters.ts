import type { ParamInfo, RackGraph, RackNoteModule } from "@/bindings"
import descriptors from "@/bindings/descriptors.json"
import type { ParamDescriptor } from "@/features/params"

// Generated from the Rust note-module tables; never maintain a second index
// order in the UI. Binding regeneration is required for this source change.
const MODULES = descriptors as unknown as {
  noteModules: Record<RackNoteModule["type"], ParamDescriptor<RackNoteModule>>
}
export function noteDescriptor(module: RackNoteModule): ParamDescriptor<RackNoteModule> {
  return MODULES.noteModules[module.type]
}
export function rackParameter(root: RackGraph, index: number): {
  module: RackNoteModule; info: ParamInfo; name: string
} | null {
  const address = root.parameters?.[index - 32]
  if (!address) return null
  let graph = root
  for (const [depth, id] of address.path.entries()) {
    const node = graph.nodes.find((node) => node.id === id)
    if (!node) return null
    if (depth < address.path.length - 1) {
      if (node.processor.type !== "rack") return null
      graph = node.processor.graph
    } else {
      if (node.processor.type !== "note" || node.processor.module.type !== address.kind) return null
      const info = noteDescriptor(node.processor.module)?.params[address.parameter]
      return info ? { module: node.processor.module, info, name: node.name } : null
    }
  }
  return null
}
export function rackParameterIndex(root: RackGraph, path: readonly number[], kind: string, parameter: number): number | null {
  const index = root.parameters?.findIndex((address) => address.kind === kind && address.parameter === parameter
    && address.path.length === path.length && address.path.every((id, i) => id === path[i])) ?? -1
  return index < 0 ? null : 32 + index
}
