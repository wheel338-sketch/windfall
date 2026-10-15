import { localizeBackendText } from "@/lib/i18n"
import { message as i18nMessage } from "@/lib/i18n"
import { plainNumber, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useEffect, useState } from "react"
import { PluginCatalog } from "./catalog"
import type { PluginManagerState, TrackId } from "@/bindings"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import { useUiStore } from "@/lib/store"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { useMixerTrackNumber } from "@/lib/store/selectors"
import { MASTER_TRACK } from "@/lib/units"
import { usePluginUi } from "./store"
import { selectChannel } from "@/features/channel-rack/channel-ops"
import { openEffect } from "@/features/mixer/effect-ops"

/**
 * Where an effect added from here goes: the requested track, else the
 * selected one, else the master, and the master when that track is gone.
 * It is named the way the mixer shows it, by its place among the strips
 * and its name, never by its id.
 */
function useDestination(): { track: TrackId; label: string } {
  const requested = usePluginUi((state) => state.track)
  const selected = useUiStore((state) => state.selectedTrack)
  const wanted = requested ?? selected ?? MASTER_TRACK
  const exists = useProjectStore((state) => state.project.mixer.tracks.some((track) => track.id === wanted))
  const track = exists ? wanted : MASTER_TRACK
  const place = useMixerTrackNumber(track)
  const name = useProjectStore((state) => state.project.mixer.tracks.find((item) => item.id === track)?.name ?? "")
  const label = track === MASTER_TRACK
    ? t("the master track")
    : place < 0
      ? i18nMessage("the {name} track", { name })
      : i18nMessage("mixer track {number}, {name}", { number: plainNumber(place), name })
  return { track, label }
}

export function PluginManager() {
  useLanguageStore((state) => state.language)
  const open = usePluginUi((state) => state.open)
  const destination = useDestination()
  const [state, setState] = useState<PluginManagerState | null>(null)
  const [search, setSearch] = useState("")
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (!open) return
    let cancelled = false
    const refresh = () =>
      backend
        .pluginsState()
        .then((state) => {
          if (!cancelled) setState(state)
        })
        .catch((error: unknown) => reportError(error, t("Could not load plugins")))
    void refresh()
    const timer = window.setInterval(() => void refresh(), 500)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [open])
  async function work(task: () => Promise<unknown>) {
    setBusy(true)
    try {
      await task()
      setState(await backend.pluginsState())
    } catch (error) {
      reportError(error, t("Plugin request failed"))
    } finally {
      setBusy(false)
    }
  }
  const entries = (state?.entries ?? []).filter((entry) =>
    `${entry.name} ${entry.vendor} ${entry.format}`
      .toLowerCase()
      .includes(search.toLowerCase())
  )
  return (
    <Dialog open={open} onOpenChange={(open) => usePluginUi.setState({ open })}>
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("Plugin manager")}</DialogTitle>
          <DialogDescription>{i18nMessage("Scan your folders, then add instruments to the rack or effects to {destination}.", { destination: destination.label })}
          </DialogDescription>
        </DialogHeader>
        <div className="flex gap-2">
          <Button
            disabled={busy || state?.scanning || backend.kind === "mock"}
            onClick={() => void work(() => backend.pluginsScan())}
          >{t("Scan plugins")}</Button>
          <Button
            variant="outline"
            disabled={busy || state?.scanning || backend.kind === "mock"}
            onClick={() =>
              void work(async () => {
                const folder = await backend.pickFolder()
                if (folder) await backend.pluginsAddFolder(folder)
              })
            }
          >{t("Add folder")}</Button>
        </div>
        <div className="flex flex-wrap gap-2"><Button variant="outline" disabled={busy || state?.scanning || backend.kind === "mock"} onClick={() => void work(() => backend.pluginsRescan(false))}>{t("Retry blocked plugins")}</Button><Button variant="outline" disabled={busy || state?.scanning || backend.kind === "mock"} onClick={() => void work(() => backend.pluginsRescan(true))}>{t("Verify every plugin again")}</Button><Button variant="outline" disabled={busy} onClick={() => void work(() => backend.pluginDatabaseForgetMissing())}>{t("Forget missing plugins")}</Button></div>
        {state?.scanning && (
          <p role="status">{i18nMessage("Scanning {completed}/{total}: {current}", { completed: state.completed, total: state.total, current: state.current ?? "" })}
          </p>
        )}
        {state?.error && (
          <Alert>
            <AlertTitle>{t("Plugin status")}</AlertTitle>
            <AlertDescription>{localizeBackendText(state.error)}</AlertDescription>
          </Alert>
        )}
        {/* One search for both lists below, so they never disagree about it. */}
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="plugin-search">{t("Search plugins")}</FieldLabel>
            <Input
              id="plugin-search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </Field>
        </FieldGroup>
        <div className="flex min-h-0 flex-col gap-3 overflow-y-auto">
          <section aria-labelledby="plugin-files-heading" className="flex flex-col gap-2">
            <div>
              <h3 id="plugin-files-heading" className="font-medium">{t("Plugin files on this computer")}</h3>
              <p className="text-muted-foreground">{t("CLAP and VST3 plugins the scan found in your plugin folders.")}</p>
            </div>
            <details>
              <summary>{i18nMessage("Plugin folders ({count})", { count: state?.folders.length ?? 0 })}</summary>
              <ul>
                {state?.folders.map((folder) => (
                  <li key={folder} className="truncate" title={folder}>
                    {folder}<Button size="sm" variant="ghost" disabled={busy || state?.scanning || backend.kind === "mock"} onClick={() => void work(() => backend.pluginsRemoveFolder(folder))}>{t("Remove folder")}</Button>
                  </li>
                ))}
              </ul>
            </details>
            {state && state.entries.length === 0 && (
              <Empty className="p-2">
                <EmptyHeader>
                  <EmptyTitle>{t("No plugin files scanned yet")}</EmptyTitle>
                  <EmptyDescription>{t("Add your plugin folder and scan it. The built-in instruments and effects are in the library below.")}</EmptyDescription>
                </EmptyHeader>
              </Empty>
            )}
            {state && state.entries.length > 0 && entries.length === 0 && (
              <p role="status" className="text-muted-foreground">{t("No plugin files match the search.")}</p>
            )}
            {entries.map((entry) => (
              <div
                key={`${entry.path}:${entry.id}`}
                className="flex items-center justify-between gap-3"
              >
                <div className="min-w-0">
                  <p className="truncate">
                    {entry.name} · {entry.vendor} · {entry.format.toUpperCase()}
                  </p>
                  <p className="text-muted-foreground">
                    {entry.instrument ? t("Instrument") : t("Effect")}
                    {entry.error ? " · " : ""}{entry.error ? localizeBackendText(entry.error) : ""}
                  </p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!entry.usable || busy || state?.scanning}
                  onClick={() =>
                    void work(async () => {
                      const result = await backend.pluginsAdd(
                        entry.path,
                        entry.id,
                        entry.instrument ? undefined : destination.track
                      )
                      receivePatch(result.patch)
                      if (entry.instrument)
                        selectChannel(result.created[0], { openSettings: true })
                      else openEffect(result.created[0])
                      usePluginUi.setState({ open: false })
                    })
                  }
                >
                  {entry.instrument ? t("Add instrument") : t("Add effect")}
                </Button>
              </div>
            ))}
          </section>
          <section aria-labelledby="plugin-library-heading" className="flex flex-col">
            <div>
              <h3 id="plugin-library-heading" className="font-medium">{t("Plugin library")}</h3>
              <p className="text-muted-foreground">{t("Built-in instruments and effects, and plugin files that passed their check.")}</p>
            </div>
            <PluginCatalog key={`${state?.completed ?? 0}:${state?.scanning ?? false}:${busy}`} track={destination.track} search={search} />
          </section>
          {state?.blocked.map((entry) => (
            <Alert key={`${entry.path}:${entry.id}`}>
              <AlertTitle>{i18nMessage("{name} is blocked", { name: entry.name })}</AlertTitle>
              <AlertDescription>
                {entry.error && localizeBackendText(entry.error)}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy || state.scanning}
                  onClick={() =>
                    void work(() => backend.pluginsScan(entry.path))
                  }
                >
                  {i18nMessage("Retry {name}", { name: entry.name })}
                </Button>
              </AlertDescription>
            </Alert>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}
