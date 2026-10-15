import { useEffect, useState } from "react"

import type { BrowserEntry } from "@/bindings"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import { Spinner } from "@/components/ui/spinner"
import { openProjectPath } from "@/lib/flows/project"
import { backend, errorSource } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { localizeBackendText, translatedText as t, message as i18nMessage } from "@/lib/i18n"

function NoBackups({ unsaved = false }: { unsaved?: boolean }) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>{t("No backups")}</EmptyTitle>
        <EmptyDescription>
          {unsaved
            ? t("This project has no backups because it has not been saved.")
            : t("No automatic backups yet. While this song has unsaved changes, Windfall copies it every five minutes into the Backup folder beside it.")}
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  )
}

/**
 * Whether a listing failed only because the Backup folder is not there yet:
 * Windfall makes it with the first backup, so a new song has none.
 */
function isMissingFolder(source: string, folder: string): boolean {
  // The shell sends a coded message, with the folder written its own way.
  if (source.startsWith("windfall-message:")) {
    try {
      const sent: unknown = JSON.parse(source.slice("windfall-message:".length))
      return typeof sent === "object" && sent !== null && "code" in sent &&
        sent.code === "browser.folderMissing"
    } catch {
      return false
    }
  }
  return source === `The folder "${folder}" does not exist.`
}

function SavedBackups({ projectPath }: { projectPath: string }) {
  const [listing, setListing] = useState<{
    entries: BrowserEntry[] | null
    error: string | null
  }>({ entries: null, error: null })

  useEffect(() => {
    let active = true
    const normalized = projectPath.replaceAll("\\", "/")
    const separator = normalized.lastIndexOf("/")
    const folder = `${normalized.slice(0, separator + 1)}Backup`
    const file = normalized.slice(separator + 1)
    const extension = file.lastIndexOf(".")
    const stem = extension > 0 ? file.slice(0, extension) : file
    const prefix = `${stem} `

    async function load() {
      try {
        const entries = (await backend.browserList(folder))
          .filter((entry) => {
            if (
              entry.kind === "folder" ||
              !entry.name.startsWith(prefix) ||
              !entry.name.endsWith(".windfall")
            ) {
              return false
            }
            const timestamp = entry.name.slice(prefix.length, -9)
            return /^\d{4}-\d{2}-\d{2} \d{2}-\d{2}-\d{2}$/.test(timestamp)
          })
          .sort((a, b) => (a.name < b.name ? 1 : a.name > b.name ? -1 : 0))
        if (active) setListing({ entries, error: null })
      } catch (error) {
        if (!active) return
        const source = errorSource(error)
        // browser_list's NotFound response; other failures remain visible.
        if (isMissingFolder(source, folder)) {
          setListing({ entries: [], error: null })
        } else {
          setListing({ entries: null, error: source })
        }
      }
    }

    void load()
    return () => {
      active = false
    }
  }, [projectPath])

  if (listing.error !== null) {
    return (
      <Alert variant="destructive">
        <AlertTitle>{t("Could not list backups")}</AlertTitle>
        {/* A long folder path wraps instead of running out of the panel. */}
        <AlertDescription className="wrap-anywhere">
          {localizeBackendText(listing.error)}
        </AlertDescription>
      </Alert>
    )
  }
  if (listing.entries === null) {
    return <Spinner aria-label={t("Loading backups")} />
  }
  if (listing.entries.length === 0) return <NoBackups />

  return (
    <>
      <p className="text-xs text-muted-foreground">
        {t("Opening a backup opens a copy and does not replace the original file.")}
      </p>
      <ul aria-label={t("Project backups")} className="flex flex-col gap-2">
        {listing.entries.map((entry) => (
          <li key={entry.path} className="flex items-center gap-2">
            <span className="min-w-0 flex-1 break-words">{entry.name}</span>
            <Button
              variant="outline"
              size="sm"
              aria-label={i18nMessage("Open {entryName}", { entryName: entry.name })}
              onClick={() => void openProjectPath(entry.path)}
            >
              {t("Open")}
            </Button>
          </li>
        ))}
      </ul>
    </>
  )
}

/** Lists existing backups only; opening uses the shared project flow. */
export function BackupsTab() {
  const path = useProjectStore((state) => state.path)
  return (
    <div data-slot="backups-browser" className="flex flex-col gap-3 p-2">
      {path === null ? (
        <NoBackups unsaved />
      ) : (
        <SavedBackups key={path} projectPath={path} />
      )}
      {/* Two backup systems sit side by side in the browser; say which is which. */}
      <p className="text-xs text-muted-foreground">
        {t("These copies are made for you. To keep copies in a folder you choose, such as one your drive client syncs, use the Folder backup tab.")}
      </p>
    </div>
  )
}
