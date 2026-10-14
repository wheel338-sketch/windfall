import { create } from "zustand"
import { toast } from "sonner"
import { localizeBackendText, message as localizedMessage, translatedText as t } from "@/lib/i18n"
import type { MessageKey } from "@/lib/i18n"
import { backend, errorSource } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { nativeUpdateApi, type UpdateApi, type UpdateInfo, type UpdateProgress } from "./native"
import { automaticChecks, checkDue, lastCheck, recordCheck, START_DELAY_MS } from "./schedule"

/**
 * One update at a time for the main window, shared by the settings section,
 * the notice an available update raises and Help → Check for updates.
 * Installing goes through the shell's shutdown: the project is saved or
 * discarded as the user says, audio and plugins stop, and a cancel or a
 * refused installer puts everything back.
 */
export type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "verified" | "installing"
export type UpdateStatus = { key: MessageKey; values?: Record<string, string | number> } | { source: string }
type UpdateState = {
  phase: UpdatePhase
  candidate: UpdateInfo | null
  progress: UpdateProgress | null
  status: UpdateStatus | null
  /** The failure as it came, translated where it is shown. */
  error: string
  /** The project has unsaved edits and installation waits for the user's choice. */
  unsaved: { token: number; path: string | null } | null
  /** The notice for an available update is on screen. */
  notice: boolean
}
const initial: UpdateState = { phase: "idle", candidate: null, progress: null, status: null, error: "", unsaved: null, notice: false }
export const useUpdateStore = create<UpdateState>(() => initial)
const set = (next: Partial<UpdateState>) => useUpdateStore.setState(next)
const state = () => useUpdateStore.getState()

let api: UpdateApi = nativeUpdateApi
let pending = false
/** The token whose shutdown the shell holds, waiting for install or cancel. */
let shutdown: number | null = null

/** The native calls the store makes; tests give it a fixture. */
export function setUpdateApi(next: UpdateApi) { api = next }
export function resetUpdates() {
  api = nativeUpdateApi; pending = false; shutdown = null
  useUpdateStore.setState(initial)
}
export function updateBusy(phase: UpdatePhase): boolean {
  return phase === "checking" || phase === "downloading" || phase === "installing"
}

/** Runs one update step; a failure rolls a held shutdown back and keeps what was verified. */
async function work(action: () => Promise<void>, quiet = false): Promise<boolean> {
  if (pending) return false
  pending = true
  if (!quiet) set({ error: "" })
  try { await action(); return true }
  catch (failure) {
    let error = errorSource(failure)
    if (shutdown !== null) {
      try { await api.cancel(shutdown) }
      catch (rollback) { error = errorSource(rollback) }
      shutdown = null; set({ unsaved: null })
    }
    const { candidate, progress } = state()
    const verified = progress?.verified === true
    set({ phase: candidate ? verified ? "verified" : "available" : "idle", progress: verified ? progress : null })
    if (!quiet) set({ error })
    else console.info("Background update check failed:", error)
    return false
  } finally { pending = false }
}

export type CheckOutcome =
  | { kind: "available"; update: UpdateInfo }
  | { kind: "current" }
  | { kind: "unconfigured"; reason: string | null }
  | { kind: "failed"; error: string }
  | { kind: "busy" }

/**
 * Asks the release host for a newer version. The background check says
 * nothing unless it finds one; the manual one reports every outcome.
 */
export async function checkForUpdates(mode: "manual" | "background"): Promise<CheckOutcome> {
  const background = mode === "background"
  if (pending || state().phase === "installing" || state().unsaved) return { kind: "busy" }
  // A background check leaves a found update, or one being fetched, alone.
  if (background && state().candidate) return { kind: "busy" }
  let outcome: CheckOutcome = { kind: "busy" }
  const done = await work(async () => {
    set(background ? { phase: "checking" } : { phase: "checking", candidate: null, progress: null, status: null })
    const result = await api.check("stable", background)
    if (!result.configured) {
      outcome = { kind: "unconfigured", reason: result.reason }
      set(background ? { phase: "idle" } : { phase: "idle", status: result.reason ? { source: result.reason } : { key: "The operator has not configured updates." } })
      return
    }
    recordCheck(Date.now())
    if (!result.update) {
      outcome = { kind: "current" }
      set(background ? { phase: "idle" } : { phase: "idle", status: { key: "You are up to date." } })
      return
    }
    outcome = { kind: "available", update: result.update }
    set({ phase: "available", candidate: result.update, progress: null, notice: true, error: "", status: { key: "Version {version} is available.", values: { version: result.update.version } } })
  }, background)
  return done ? outcome : { kind: "failed", error: state().error }
}

/** Fetches the package and has the shell verify its signature; nothing runs before that. */
export function downloadUpdate(): Promise<boolean> {
  return work(downloadVerified)
}
async function downloadVerified() {
  const candidate = state().candidate
  if (!candidate) return
  set({ phase: "downloading", progress: null })
  let verified = false
  await api.download(candidate.token, (next) => {
    if (state().candidate?.token !== candidate.token) return
    verified = next.verified
    set({ progress: next })
  })
  if (state().candidate?.token !== candidate.token) return
  if (!verified) throw new Error(localizedMessage("The package was not verified. Nothing was installed.", {}, "en"))
  set({ phase: "verified" })
}
async function beginInstall() {
  const candidate = state().candidate
  if (!candidate) return
  set({ phase: "installing" })
  const snapshot = await api.begin(candidate.token)
  shutdown = candidate.token
  if (snapshot.dirty) set({ unsaved: { token: candidate.token, path: snapshot.path } })
  else await install(candidate.token, "discard")
}
async function install(token: number, decision: "save" | "discard", path?: string) {
  if (decision === "save" && !path) {
    const picked = await backend.pickProjectSavePath(useProjectStore.getState().project.settings.name)
    if (picked === null) { await api.cancel(token); shutdown = null; set({ unsaved: null, phase: "verified" }); return }
    path = picked
  }
  // On Windows the installer takes over here and Windfall restarts as the new version.
  await api.install(token, decision, path)
  shutdown = null
  set({ unsaved: null, status: { key: "Installed. Restart Windfall to use the update." }, phase: "idle", candidate: null, notice: false })
}

/** Install and restart, once the package is verified. */
export function startInstall(): Promise<boolean> {
  return work(beginInstall)
}
/** The notice's Install now: download and verify if that is still to do, then install. */
export function installNow(): Promise<boolean> {
  return work(async () => {
    if (state().phase !== "verified") await downloadVerified()
    if (state().phase === "verified") await beginInstall()
  })
}
/** The answer to the unsaved-project prompt. Cancel thaws the project and keeps the verified package. */
export function resolveUnsaved(decision: "save" | "discard" | "cancel"): Promise<boolean> {
  return work(async () => {
    const unsaved = state().unsaved
    if (!unsaved) return
    if (decision !== "cancel") { await install(unsaved.token, decision, unsaved.path ?? undefined); return }
    if (shutdown !== null) await api.cancel(shutdown)
    shutdown = null; set({ unsaved: null, phase: "verified" })
  })
}
/** Later: the update stays in Settings → Updates and in Help → Check for updates. */
export function dismissNotice() { set({ notice: false }) }
/** Lets the shell go if the window goes while it waits for an answer. */
export function releaseUpdate() {
  if (shutdown !== null) void api.cancel(shutdown).catch(() => {})
  shutdown = null
}

/** Help → Check for updates: the notice for an update, a toast for anything else. */
export async function checkFromMenu() {
  const outcome = await checkForUpdates("manual")
  switch (outcome.kind) {
    case "available": set({ notice: true }); break
    case "current": toast(t("You are up to date.")); break
    case "unconfigured": toast(outcome.reason ? localizeBackendText(outcome.reason) : t("The operator has not configured updates.")); break
    case "failed": toast.error(t("Could not check for updates"), { description: localizeBackendText(outcome.error) }); break
    case "busy": set({ notice: state().candidate !== null }); break
  }
}

/**
 * The start-up check: once the window has been up a few seconds, if it is
 * on and a day has passed since the last one. It never holds anything up
 * and fails quietly. Returns its cancel.
 */
export function startAutomaticChecks({ native = backend.kind === "tauri", delay = START_DELAY_MS } = {}): () => void {
  if (!native || !automaticChecks() || !checkDue(Date.now(), lastCheck())) return () => {}
  const timer = setTimeout(() => {
    if (automaticChecks() && checkDue(Date.now(), lastCheck())) void checkForUpdates("background")
  }, delay)
  return () => clearTimeout(timer)
}
