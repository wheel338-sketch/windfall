import { useEffect, useState } from "react"
import { PluginCatalog } from "./catalog"
import type { PluginManagerState } from "@/bindings"
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
import { receivePatch } from "@/lib/store/project"
import { usePluginUi } from "./store"
import { selectChannel } from "@/features/channel-rack/channel-ops"
import { openEffect } from "@/features/mixer/effect-ops"

export function PluginManager() {
  const open = usePluginUi((state) => state.open)
  const requestedTrack = usePluginUi((state) => state.track)
  const selectedTrack = useUiStore((state) => state.selectedTrack)
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
        .catch((error: unknown) => reportError(error, "Could not load plugins"))
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
      reportError(error, "Plugin request failed")
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
          <DialogTitle>Plugin manager</DialogTitle>
          <DialogDescription>
            Scan your folders, then add instruments to the rack or effects to
            mixer track {requestedTrack ?? selectedTrack ?? 0}.
          </DialogDescription>
        </DialogHeader>
        <div className="flex gap-2">
          <Button
            disabled={busy || state?.scanning || backend.kind === "mock"}
            onClick={() => void work(() => backend.pluginsScan())}
          >
            Scan plugins
          </Button>
          <Button
            variant="outline"
            disabled={busy || state?.scanning || backend.kind === "mock"}
            onClick={() =>
              void work(async () => {
                const folder = await backend.pickFolder()
                if (folder) await backend.pluginsAddFolder(folder)
              })
            }
          >
            Add folder
          </Button>
        </div>
        <div className="flex flex-wrap gap-2"><Button variant="outline" disabled={busy || state?.scanning || backend.kind === "mock"} onClick={() => void work(() => backend.pluginsRescan(false))}>Retry blocked plugins</Button><Button variant="outline" disabled={busy || state?.scanning || backend.kind === "mock"} onClick={() => void work(() => backend.pluginsRescan(true))}>Verify every plugin again</Button><Button variant="outline" disabled={busy} onClick={() => void work(() => backend.pluginDatabaseForgetMissing())}>Forget missing plugins</Button></div>
        {state?.scanning && (
          <p role="status">
            Scanning {state.completed}/{state.total}: {state.current}
          </p>
        )}
        {state?.error && (
          <Alert>
            <AlertTitle>Plugin status</AlertTitle>
            <AlertDescription>{state.error}</AlertDescription>
          </Alert>
        )}
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="plugin-search">Search plugins</FieldLabel>
            <Input
              id="plugin-search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </Field>
        </FieldGroup>
        <div className="flex min-h-0 flex-col gap-3 overflow-y-auto">
          <details>
            <summary>Plugin folders ({state?.folders.length ?? 0})</summary>
            <ul>
              {state?.folders.map((folder) => (
                <li key={folder} className="truncate" title={folder}>
                  {folder}<Button size="sm" variant="ghost" disabled={busy || state?.scanning || backend.kind === "mock"} onClick={() => void work(() => backend.pluginsRemoveFolder(folder))}>Remove folder</Button>
                </li>
              ))}
            </ul>
          </details>
          {entries.length === 0 && (
            <Empty>
              <EmptyHeader>
                <EmptyTitle>No plugins found</EmptyTitle>
                <EmptyDescription>
                  Add your plugin folder and scan it.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
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
                  {entry.instrument ? "Instrument" : "Effect"}
                  {entry.error ? ` · ${entry.error}` : ""}
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
                      entry.instrument
                        ? undefined
                        : (requestedTrack ?? selectedTrack ?? 0)
                    )
                    receivePatch(result.patch)
                    if (entry.instrument)
                      selectChannel(result.created[0], { openSettings: true })
                    else openEffect(result.created[0])
                    usePluginUi.setState({ open: false })
                  })
                }
              >
                Add {entry.instrument ? "instrument" : "effect"}
              </Button>
            </div>
          ))}
          <PluginCatalog key={`${state?.completed ?? 0}:${state?.scanning ?? false}:${busy}`} />
          {state?.blocked.map((entry) => (
            <Alert key={`${entry.path}:${entry.id}`}>
              <AlertTitle>{entry.name} is blocked</AlertTitle>
              <AlertDescription>
                {entry.error}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy || state.scanning}
                  onClick={() =>
                    void work(() => backend.pluginsScan(entry.path))
                  }
                >
                  Retry {entry.name}
                </Button>
              </AlertDescription>
            </Alert>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}
