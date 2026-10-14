/** When Windfall looks for an update by itself: at start-up, at most once a day, unless turned off. */
export const AUTO_CHECK_KEY = "windfall.update-auto-check"
export const LAST_CHECK_KEY = "windfall.update-last-check"
export const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000
/** Start-up work (the project, the audio device, the panels) comes first. */
export const START_DELAY_MS = 8_000

export function automaticChecks(): boolean {
  try { return localStorage.getItem(AUTO_CHECK_KEY) !== "off" }
  catch { return true }
}
export function setAutomaticChecks(on: boolean) {
  try { localStorage.setItem(AUTO_CHECK_KEY, on ? "on" : "off") } catch { /* The session keeps the choice. */ }
}
export function lastCheck(): number | null {
  try {
    const value = Number(localStorage.getItem(LAST_CHECK_KEY))
    return Number.isFinite(value) && value > 0 ? value : null
  } catch { return null }
}
export function recordCheck(now: number) {
  try { localStorage.setItem(LAST_CHECK_KEY, String(now)) } catch { /* Checked again next start. */ }
}
/** Due a day after the last check. A last check in the future (a clock set back) does not hold it off. */
export function checkDue(now: number, last: number | null): boolean {
  return last === null || last > now || now - last >= CHECK_INTERVAL_MS
}

/** The first lines of the release notes, without Markdown marks, for the notice. */
export function notesSummary(notes: string, limit = 280): string {
  const lines = notes
    .split(/\r?\n/)
    .map((line) => line.replace(/^\s*(#{1,6}\s+|[-*+]\s+|>\s?)/, "").replace(/[*_`]/g, "").trim())
    .filter((line) => line.length > 0)
  let summary = ""
  for (const line of lines) {
    const next = summary ? `${summary}\n${line}` : line
    if (next.length > limit) {
      if (!summary) summary = `${line.slice(0, limit - 1).trimEnd()}…`
      else summary = `${summary}\n…`
      break
    }
    summary = next
  }
  return summary
}
