import { create } from "zustand"

import type { ChannelId, Pattern, TimeSignature } from "@/bindings"
import { backend } from "@/lib/ipc"
import { onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { onProjectReplaced } from "@/lib/store/replaced"

import { PAPERS, type PaperSize } from "./score/layout"
import type { ScorePage } from "./score/draw"
import { scorePdf } from "./score/pdf"
import { pageSvg } from "./score/svg"
import { currentSession } from "./session"
import { buildChannelMusicXml } from "./sheet-music"

/**
 * What the sheet export engraves: the pattern and channel as they were when
 * the dialog opened. Later edits do not reach an open dialog; closing and
 * opening it again takes the current notes.
 */
export type SheetRequest = {
  serial: number
  pattern: Pattern
  channel: ChannelId
  channelName: string
  /** The project's base meter, tempo and author at that moment. */
  baseSignature: TimeSignature
  tempoBpm: number
  author: string
}

export const useSheetExport = create<{ request: SheetRequest | null }>(() => ({
  request: null,
}))

let serial = 0

export function closeSheetExport(): void {
  useSheetExport.setState({ request: null })
}

onProjectReplaced(closeSheetExport)
onHistoryNavigation(closeSheetExport)

/** Opens the export dialog for the open pattern channel. Edits nothing. */
export function exportSheetMusic(): void {
  const context = currentSession()?.editor.context
  if (!context) return
  const { project } = useProjectStore.getState()
  const pattern = project.patterns.find(
    (item) => item.id === context.pattern.id
  )
  const channel = project.channels.find((item) => item.id === context.channel)
  if (!pattern || !channel) return
  useSheetExport.setState({
    request: {
      serial: ++serial,
      pattern,
      channel: channel.id,
      channelName: channel.name,
      baseSignature: project.settings.timeSignature,
      tempoBpm: project.settings.tempoBpm,
      author: project.settings.author ?? "",
    },
  })
}

function suggestedName(request: SheetRequest): string {
  return `${request.pattern.name} - ${request.channelName}`
}

/** Saves the engraved pages as a PDF. Resolves to the path, or null on cancel. */
export function saveSheetPdf(
  request: SheetRequest,
  pages: readonly ScorePage[]
): Promise<string | null> {
  const pdf = scorePdf(pages, {
    title: suggestedName(request),
    author: request.author || undefined,
  })
  return backend.sheetMusicSave(suggestedName(request), pdf, "pdf")
}

/** Saves the same notes as MusicXML, for a notation program to engrave. */
export function saveSheetMusicXml(
  request: SheetRequest
): Promise<string | null> {
  const xml = buildChannelMusicXml(
    request.pattern,
    request.channel,
    request.channelName
  )
  return backend.sheetMusicSave(suggestedName(request), xml, "musicxml")
}

const PRINT_ATTRIBUTE = "data-score-print"

/**
 * Sends the pages to the system print dialog. A print-only container holds
 * one SVG per sheet; everything else is hidden while printing, and the
 * container is removed again whether or not anything was printed.
 */
export function printSheet(
  pages: readonly ScorePage[],
  paper: PaperSize
): void {
  if (typeof document === "undefined" || typeof window === "undefined") return
  document
    .querySelectorAll(`[${PRINT_ATTRIBUTE}]`)
    .forEach((node) => node.remove())
  const style = document.createElement("style")
  style.setAttribute(PRINT_ATTRIBUTE, "style")
  // A print stylesheet, not a caption.
  style.append([
    `@page { size: ${PAPERS[paper].css} portrait; margin: 0; }`,
    `div[${PRINT_ATTRIBUTE}] { display: none; }`,
    "@media print {",
    `  body > *:not([${PRINT_ATTRIBUTE}]) { display: none !important; }`,
    `  div[${PRINT_ATTRIBUTE}] { display: block; }`,
    `  div[${PRINT_ATTRIBUTE}] > div { break-after: page; page-break-after: always; }`,
    `  div[${PRINT_ATTRIBUTE}] > div:last-child { break-after: auto; page-break-after: auto; }`,
    `  div[${PRINT_ATTRIBUTE}] svg { display: block; width: 100%; height: auto; }`,
    "}",
  ].join("\n"))
  const sheets = document.createElement("div")
  sheets.setAttribute(PRINT_ATTRIBUTE, "pages")
  for (const page of pages) {
    const sheet = document.createElement("div")
    sheet.innerHTML = pageSvg(page)
    sheets.append(sheet)
  }
  const cleanup = () => {
    window.removeEventListener("afterprint", cleanup)
    style.remove()
    sheets.remove()
  }
  window.addEventListener("afterprint", cleanup)
  document.head.append(style)
  document.body.append(sheets)
  try {
    window.print()
  } catch (error) {
    cleanup()
    throw error
  }
  // Where printing blocks until the dialog closes, `afterprint` has fired
  // by now. Where it does not, the listener cleans up when it does.
}
