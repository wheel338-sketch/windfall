import type { ParamInfo } from "@/bindings"

import type { ParamDescriptor } from "./descriptors"
import { translatedText as t } from "@/lib/i18n"

export type GroupedParam = {
  info: ParamInfo
  /** The setting's name without what the group's title already says. */
  label: string
}

export type ParamGroupPlan = {
  key: string
  /** Null when the descriptor is one flat list and needs no heading. */
  title: string | null
  params: GroupedParam[]
}

/** Words that are written in capitals. */
const ACRONYMS: Record<string, string> = { lfo: "LFO", eq: "EQ" }

/** `"lowShelf"` to `["low", "shelf"]`, `"peak1"` to `["peak", "1"]`. */
function words(segment: string): string[] {
  return segment
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/([A-Za-z])(\d)/g, "$1 $2")
    .toLowerCase()
    .split(" ")
}

function sentence(parts: string[]): string {
  const text = parts.map((word) => ACRONYMS[word] ?? word).join(" ")
  return text.charAt(0).toUpperCase() + text.slice(1)
}

/**
 * A heading from the path of a group: `["ampEnvelope"]` is "Amp envelope",
 * `["oscillators", "0"]` is "Oscillator 1" and `["lfos", "1"]` is "LFO 2".
 */
export function humanizePath(path: readonly string[]): string {
  const parts: string[] = []
  for (const segment of path) {
    if (/^\d+$/.test(segment)) {
      // A place in a list: the list's name loses its plural and gets a
      // number that counts from one.
      const last = parts.length - 1
      if (last >= 0) parts[last] = parts[last].replace(/s$/, "")
      parts.push(String(Number(segment) + 1))
    } else {
      parts.push(...words(segment))
    }
  }
  return sentence(parts)
}

function commonPrefix(lists: string[][]): string[] {
  const [first, ...rest] = lists
  if (!first) return []
  let length = first.length
  for (const list of rest) {
    let same = 0
    while (same < length && same < list.length && list[same] === first[same]) {
      same += 1
    }
    length = same
  }
  return first.slice(0, length)
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1)
}

function withoutPrefix(name: string, prefix: string): string | null {
  if (!name.toLowerCase().startsWith(`${prefix.toLowerCase()} `)) return null
  return capitalize(name.slice(prefix.length + 1))
}

/**
 * Names for the controls of one group. What every name starts with is said
 * once by the heading, and so is the heading itself: in "Oscillator 1",
 * "Osc 1 level" becomes "Level", and under "Filter", "Filter mode" becomes
 * "Mode". A toggle whose whole name was the shared part reads "Enabled".
 */
export function shortLabels(
  infos: readonly ParamInfo[],
  title: string | null
): string[] {
  const names = infos.map((info) => info.name.split(" "))
  const shared = infos.length > 1 ? commonPrefix(names).join(" ") : ""
  return infos.map((info) => {
    if (shared !== "") {
      if (info.name === shared) {
        return info.kind === "toggle" ? t("Enabled") : shared
      }
      const rest = withoutPrefix(info.name, shared)
      if (rest !== null) return rest
    }
    return (title !== null && withoutPrefix(info.name, title)) || info.name
  })
}

/**
 * Sorts the settings of a descriptor into groups by where they live in the
 * settings object: everything under `filter.` is one group, everything under
 * `oscillators.0.` another. Settings at the top level form a group for each
 * run of them in the table, named after the word their ids share ("Unison"
 * for `unisonVoices` and `unisonSpread`) and "General" otherwise. Groups
 * come in the order the table first mentions them.
 */
export function paramGroups(descriptor: ParamDescriptor): ParamGroupPlan[] {
  const groups: { key: string; path: string[]; infos: ParamInfo[] }[] = []
  let run: (typeof groups)[number] | null = null

  for (const info of descriptor.params) {
    const path = info.id.split(".").slice(0, -1)
    if (path.length === 0) {
      if (!run) {
        run = { key: `top:${groups.length}`, path: [], infos: [] }
        groups.push(run)
      }
      run.infos.push(info)
      continue
    }
    run = null
    const key = path.join(".")
    let group = groups.find((item) => item.key === key)
    if (!group) {
      group = { key, path, infos: [] }
      groups.push(group)
    }
    group.infos.push(info)
  }

  const flat = groups.length === 1 && groups[0].path.length === 0
  return groups.map((group) => {
    let title: string | null
    if (group.path.length > 0) {
      title = humanizePath(group.path)
    } else if (flat) {
      title = null
    } else {
      const shared = commonPrefix(group.infos.map((info) => words(info.id)))
      title =
        group.infos.length > 1 && shared.length > 0
          ? sentence(shared)
          : t("General")
    }
    const labels = shortLabels(group.infos, title)
    return {
      key: group.key,
      title,
      params: group.infos.map((info, index) => ({
        info,
        label: labels[index],
      })),
    }
  })
}
