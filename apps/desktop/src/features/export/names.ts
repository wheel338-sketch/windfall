import type { ExportFormat } from "@/bindings"
import { fileName } from "@/lib/time"

/** The longest tail an export may be given, in seconds. */
export const MAX_TAIL_SECS = 30
/** The most passes through the pattern one export renders. */
export const MAX_PATTERN_LOOPS = 64

/**
 * A typed path as it is sent to the shell: without the spaces around it
 * and without dots or spaces at its end. A file name cannot end in either
 * on Windows, and the shell adds ".wav" to a name with no extension, so
 * "take 1." would come out as "take 1..wav".
 */
export function cleanExportPath(path: string): string {
  return path.trim().replace(/[.\s]+$/, "")
}

/** The seconds a typed tail stands for, or null when it cannot be used. */
export function parseTailSecs(text: string): number | null {
  if (text.trim() === "") return null
  const seconds = Number(text)
  return Number.isFinite(seconds) && seconds >= 0 && seconds <= MAX_TAIL_SECS
    ? seconds
    : null
}

/** The passes a typed count stands for, or null when it cannot be used. */
export function parsePatternLoops(text: string): number | null {
  if (text.trim() === "") return null
  const loops = Number(text)
  return Number.isInteger(loops) && loops >= 1 && loops <= MAX_PATTERN_LOOPS
    ? loops
    : null
}

/**
 * The name of the file an export wrote. The shell adds the format's extension
 * to a path with no extension, and reports progress under the submitted path.
 */
export function exportedName(
  path: string,
  format: ExportFormat = "wav"
): string {
  const name = fileName(path)
  return /\.[^.]+$/.test(name) ? name : `${name}.${format}`
}

/** Keep a chosen audio file name in step with the format control. */
export function replaceExportExtension(
  path: string,
  format: ExportFormat
): string {
  return path.replace(/\.(wav|flac|ogg|mp3|wv|m4a)$/i, `.${format}`)
}
