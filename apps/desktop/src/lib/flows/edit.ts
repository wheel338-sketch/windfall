import type { PatternId } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { askConfirm, askText } from "@/lib/store/prompts"
import { selectedPatternId } from "@/lib/store/selectors"
import { setTransportPattern, useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

function patterns() {
  return useProjectStore.getState().project.patterns
}

/** The pattern being edited, read right now. */
export function currentPatternId(): PatternId | null {
  return selectedPatternId(
    useProjectStore.getState().project,
    useTransportStore.getState().pattern
  )
}

export async function addPattern(): Promise<void> {
  const result = await dispatch({ type: "addPattern" })
  if (result) await setTransportPattern(result.created[0])
}

export async function duplicatePattern(id = currentPatternId()): Promise<void> {
  if (id === null) return
  const result = await dispatch({ type: "duplicatePattern", id })
  if (result) await setTransportPattern(result.created[0])
}

export async function renamePattern(id = currentPatternId()): Promise<void> {
  const pattern = patterns().find((item) => item.id === id)
  if (!pattern) return
  const name = await askText({
    title: t("Rename pattern"),
    label: t("Name"),
    initial: pattern.name,
    submitLabel: t("Rename"),
  })
  if (name === null || name === pattern.name) return
  await dispatch({ type: "updatePattern", id: pattern.id, patch: { name } })
}

/**
 * What deleting a pattern takes with it, in words, or null for an empty
 * pattern nothing plays: that one goes without a question.
 */
export function patternLoss(id: PatternId): string | null {
  const { project } = useProjectStore.getState()
  const pattern = project.patterns.find((item) => item.id === id)
  if (!pattern) return null
  const notes = pattern.lanes.reduce(
    (total, lane) => total + lane.notes.length,
    0
  )
  const clips = project.playlist.clips.filter(
    (clip) => clip.content.type === "pattern" && clip.content.pattern === id
  ).length
  if (notes === 0 && clips === 0) return null
  const has = message("It has {count} notes", { count: notes })
  if (clips === 0) return message("{has}. Undo brings the pattern back.", { has })
  const used =
    clips === 1
      ? t("1 clip on the playlist plays it and is deleted with it")
      : message("{clips} clips on the playlist play it and are deleted with it", { clips: plainNumber(clips) })
  const lost = notes === 0 ? used : message("{has}, and {used}", { has, used })
  return message("{lost}. Undo brings everything back.", { lost })
}

export async function deletePattern(id = currentPatternId()): Promise<void> {
  if (id === null) return
  const pattern = patterns().find((item) => item.id === id)
  if (!pattern) return
  const loss = patternLoss(id)
  if (loss !== null) {
    const choice = await askConfirm({
      title: message("Delete {patternName}?", { patternName: pattern.name }),
      description: loss,
      choices: [
        { id: "delete", label: t("Delete pattern"), variant: "destructive" },
      ],
    })
    if (choice !== "delete") return
  }
  const index = patterns().findIndex((pattern) => pattern.id === id)
  const neighbor = patterns()[index + 1] ?? patterns()[index - 1]
  const wasSelected = id === currentPatternId()
  const result = await dispatch({ type: "removePattern", id })
  if (result && wasSelected && neighbor) await setTransportPattern(neighbor.id)
}

/** Selects the pattern after (1) or before (-1) the current one. */
export async function stepPattern(delta: 1 | -1): Promise<void> {
  const list = patterns()
  const index = list.findIndex((pattern) => pattern.id === currentPatternId())
  const next = list[index + delta]
  if (next) await setTransportPattern(next.id)
}

/** Adds an empty sampler channel, selects it and brings the rack forward. */
export async function addChannel(): Promise<void> {
  const result = await dispatch({ type: "addChannel" })
  if (!result) return
  const ui = useUiStore.getState()
  ui.selectChannel(result.created[0])
  ui.showCenterTab("channelRack")
}
