import { useEffect, useState } from "react"
import type { BackupVersion } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Empty, EmptyHeader, EmptyTitle, EmptyDescription } from "@/components/ui/empty"
import { openProjectPath } from "@/lib/flows/project"
import { backend, errorMessage } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, useProjectGeneration } from "@/lib/store/replaced"
import { EMPTY_BACKUP, rememberBackup, useBackupConfig, useBackupJob, type FolderBackupConfig } from "./store"

export function FolderBackupPanel() {
  const path = useProjectStore((state) => state.path)
  const generation = useProjectGeneration()
  const saved = useBackupConfig((state) => path ? state.projects[path] : undefined)
  const [unsaved, setUnsaved] = useState<FolderBackupConfig>(EMPTY_BACKUP)
  const config = saved ?? unsaved
  const [rows, setRows] = useState<BackupVersion[]>([])
  const job = useBackupJob()
  function configure(next: FolderBackupConfig) { setUnsaved(next); rememberBackup(path, next) }
  useEffect(() => {
    let live = true
    void Promise.resolve().then(async () => {
      const rows = config.folder ? await backend.backupVersions(config.folder) : []
      if (live) setRows(rows)
    }).catch((error: unknown) => { if (live) useBackupJob.setState({ error: errorMessage(error) }) })
    return () => { live = false }
  }, [config.folder, generation, job.result])
  async function choose() {
    const localGeneration = getProjectGeneration()
    try {
      const folder = await backend.pickFolder()
      if (!folder) return
      const versions = await backend.backupVersions(folder)
      if (localGeneration !== getProjectGeneration()) return
      setRows(versions); configure({ ...config, folder, head: versions.at(-1)?.id ?? null, afterSave: false })
      useBackupJob.setState({ error: null, result: null })
    } catch (error) { useBackupJob.setState({ error: errorMessage(error) }) }
  }
  async function refresh() {
    if (!config.folder) return
    try { const rows = await backend.backupVersions(config.folder); setRows(rows); configure({ ...config, head: rows.at(-1)?.id ?? null }); useBackupJob.setState({ error: null }) }
    catch (error) { useBackupJob.setState({ error: errorMessage(error) }) }
  }
  async function save() {
    if (!config.folder || job.busy) return
    const localGeneration = getProjectGeneration()
    useBackupJob.setState({ busy: true, error: null, result: null })
    try {
      const catalog = await backend.loopCatalog()
      if (localGeneration !== getProjectGeneration()) throw new Error("The project changed before backup started.")
      const row = await backend.backupSave(config.folder, config.head, config.keep, catalog.generation)
      if (localGeneration === getProjectGeneration()) configure({ ...config, head: row.id })
      useBackupJob.setState({ result: "Portable backup created and checksum recorded." })
    } catch (error) { useBackupJob.setState({ error: errorMessage(error) }) }
    finally { useBackupJob.setState({ busy: false }) }
  }
  async function restore(row: BackupVersion) {
    if (!config.folder || job.busy) return
    useBackupJob.setState({ busy: true, error: null, result: null })
    try {
      const target = await backend.pickProjectArchivePath(`Restored ${row.id.slice(0, 8)}`)
      if (!target) return
      const restored = await backend.backupRestore(config.folder, row.id, target)
      useBackupJob.setState({ result: `Verified backup restored to ${restored}` })
      await openProjectPath(restored)
    } catch (error) { useBackupJob.setState({ error: errorMessage(error) }) }
    finally { useBackupJob.setState({ busy: false }) }
  }
  const native = backend.kind === "tauri"
  return <div className="flex flex-col gap-4 p-3">
    <FieldSet><FieldLegend>Optional folder backup</FieldLegend><FieldGroup>
      <Field><FieldDescription>Choose a dedicated folder for this project, including a folder synced by your own drive client. Windfall has no cloud account or upload service. No backup runs until you request it or enable backup after Save.</FieldDescription></Field>
      <Field><FieldLabel>Backup folder</FieldLabel><FieldDescription>{config.folder ?? "No folder selected"}</FieldDescription><Button variant="outline" disabled={!native || job.busy} onClick={() => void choose()}>Choose backup folder</Button></Field>
      <Field><FieldLabel htmlFor="backup-keep">Versions to retain</FieldLabel><Input id="backup-keep" type="number" min={1} max={1000} value={config.keep} disabled={job.busy} onChange={(event) => { const keep = Number(event.target.value); if (Number.isInteger(keep) && keep >= 1 && keep <= 1000) configure({ ...config, keep }) }} /></Field>
      <Field orientation="horizontal"><Checkbox id="backup-after-save" checked={config.afterSave} disabled={!native || !path || !config.folder || job.busy} onCheckedChange={(checked) => configure({ ...config, afterSave: checked === true })} /><FieldLabel htmlFor="backup-after-save">Back up this project after Save</FieldLabel></Field>
      {!path && <FieldDescription>Save the project once before enabling automatic backup. Manual backup also works for an unsaved project.</FieldDescription>}
      <div className="flex flex-wrap gap-2"><Button disabled={!native || !config.folder || job.busy} onClick={() => void save()}>Back up now</Button><Button variant="outline" disabled={!config.folder || job.busy} onClick={() => void refresh()}>Refresh history</Button><Button variant="outline" disabled={!job.busy} onClick={() => void backend.backupCancel().catch((error: unknown) => useBackupJob.setState({ error: errorMessage(error) }))}>Cancel backup</Button></div>
    </FieldGroup></FieldSet>
    {!native && <Alert><AlertDescription>Folder backups require the Windfall desktop app.</AlertDescription></Alert>}
    {job.error && <Alert variant="destructive"><AlertDescription>{job.error}</AlertDescription></Alert>}
    {job.result && <p role="status">{job.result}</p>}
    {!rows.length ? <Empty><EmptyHeader><EmptyTitle>No backup versions</EmptyTitle><EmptyDescription>Choose a folder and create a portable backup.</EmptyDescription></EmptyHeader></Empty> : <ul className="flex flex-col gap-3">{rows.slice().reverse().map((row) => <li key={row.id} className="flex flex-col gap-1"><span>{new Date(row.createdMs).toLocaleString()} · {row.bytes.toLocaleString()} bytes · {row.id.slice(0, 8)}</span><span className="break-all text-muted-foreground">SHA-256 {row.checksum}</span><Button variant="outline" disabled={job.busy} onClick={() => void restore(row)}>Restore {row.id.slice(0, 8)}</Button></li>)}</ul>}
  </div>
}
