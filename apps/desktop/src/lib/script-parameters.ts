import type { ParamInfo, Project } from "@/bindings"

export const SCRIPT_PARAMETER_BASE = 0x80000000

/** Read declarations from an already checked saved revision; never execute it. */
export function scriptParameter(project: Pick<Project, "settings">, channel: number, address: number) {
  const library = project.settings.scripts
  const entry = library?.parameterAddresses?.[address - SCRIPT_PARAMETER_BASE]
  if (!entry || entry.channel !== channel) return null
  const binding = library?.channels.find(b => b.channel === channel && b.script.id === entry.script)
  const asset = binding && library?.assets.find(a => a.reference.id === binding.script.id && a.reference.revision === binding.script.revision)
  if (!asset || !binding) return null
  const declarations = asset.source.split(/\r?\n/).map(line => line.split("#")[0].trim().split(/\s+/))
    .filter(words => words[0] === "param" && words.length === 5)
  const index = declarations.findIndex(words => words[1] === entry.name)
  if (index < 0) return null
  const words = declarations[index]
  const info: ParamInfo = { id: entry.name, name: entry.name, min: Number(words[2]), max: Number(words[3]),
    default: Number(words[4]), kind: "float", scale: "linear", unit: "none", choices: [] }
  return { info, stored: binding.parameters[index] ?? info.default, index }
}

export function scriptParameterTargets(project: Pick<Project, "settings">) {
  return (project.settings.scripts?.parameterAddresses ?? []).flatMap((entry, index) => {
    const param = SCRIPT_PARAMETER_BASE + index
    return scriptParameter(project, entry.channel, param) ? [{ type: "instrumentParam" as const, channel: entry.channel, param }] : []
  })
}
