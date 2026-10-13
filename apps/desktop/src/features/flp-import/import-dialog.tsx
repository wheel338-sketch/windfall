import { useEffect, useRef, useState } from "react"
import { toast } from "sonner"
import type { FlpImportOptions, FlpImportPreview } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import {
  Collapsible,
  CollapsibleTrigger,
  CollapsibleContent,
} from "@/components/ui/collapsible"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldGroup,
  FieldLabel,
  FieldDescription,
} from "@/components/ui/field"
import { backend, errorMessage } from "@/lib/ipc"
import { confirmDiscardChanges } from "@/lib/flows/project"
import { loadSnapshot } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { fileName } from "@/lib/time"
import { translatedText as t, message } from "@/lib/i18n"

function Review({ preview }: { preview: FlpImportPreview }) {
  return (
    <section aria-label={t("Import report")} className="flex flex-col gap-3">
      <p>
        {message("{name} · FL Studio", { name: preview.name })}{" "}
        {preview.report.flVersion ?? t("unknown version")}
      </p>
      <p>
        {t("Unsupported sounds remain silent or bypassed. Retained plugin states are saved with the converted project; mapped processors may sound different.")}
      </p>
      <div className="flex flex-wrap gap-2">
        <Badge variant="secondary">
          {preview.missingSamples.length}{" "}{t("missing samples")}
        </Badge>
        <Badge variant="secondary">
          {preview.retainedPlugins}{" "}{t("retained plugin states")}
        </Badge>
      </div>
      {preview.report.categories
        .filter(
          (c) => c.exact + c.approximated + c.placeholders + c.dropped > 0
        )
        .map((c) => (
          <Collapsible key={c.section}>
            <CollapsibleTrigger
              render={
                <Button
                  variant="ghost"
                  className="h-auto w-full justify-between"
                />
              }
            >
              <span>{c.title}</span>
              <span>
                {c.exact}{" "}{t("exact ·")}{" "}{c.approximated}{" "}{t("approximate ·")}{" "}
                {c.placeholders}{" "}{t("placeholders ·")}{" "}{c.dropped}{" "}{t("omitted")}
              </span>
            </CollapsibleTrigger>
            <CollapsibleContent>
              <ul className="flex flex-col gap-2 pl-4">
                {c.lines.map((line, i) => (
                  <li key={i}>
                    {line.count > 1 ? `${line.count} × ` : ""}
                    {line.text}
                  </li>
                ))}
                {c.moreLines > 0 && (
                  <li>{c.moreLines}{" "}{t("additional details omitted.")}</li>
                )}
              </ul>
            </CollapsibleContent>
          </Collapsible>
        ))}
      <Collapsible>
        <CollapsibleTrigger render={<Button variant="outline" />}>
          {t("Diagnostics and missing samples (")}
          {preview.warnings.length +
            preview.report.readProblems.length +
            preview.report.unknownEventIds.length}
          )
        </CollapsibleTrigger>
        <CollapsibleContent>
          <ul className="flex flex-col gap-2 pt-2">
            {[...preview.report.readProblems, ...preview.warnings].map(
              (line, i) => (
                <li key={i} className="wrap-anywhere">
                  {line}
                </li>
              )
            )}
          </ul>
          {preview.report.unknownEventIds.length > 0 && (
            <p>
              {t("Unknown event ids:")}{" "}{preview.report.unknownEventIds.join(", ")}
            </p>
          )}
        </CollapsibleContent>
      </Collapsible>
    </section>
  )
}

function ImportForm({
  onDone,
  onBusyChange,
}: {
  onDone(): void
  onBusyChange(busy: boolean): void
}) {
  const [path, setPath] = useState<string | null>(null)
  const [preview, setPreview] = useState<FlpImportPreview | null>(null)
  const [options, setOptions] = useState<FlpImportOptions>({
    factoryDataDir: null,
    userDataDir: null,
    sampleSearchFolders: [],
  })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const token = useRef<number | null>(null)
  const mounted = useRef(true)
  function working(busy: boolean) {
    setBusy(busy)
    onBusyChange(busy)
  }
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      if (token.current !== null)
        void backend.flpCancel(token.current).catch(() => {})
    }
  }, [])

  async function review(nextPath: string, nextOptions = options) {
    working(true)
    setError(null)
    try {
      if (token.current !== null) await backend.flpCancel(token.current)
      token.current = null
      setPreview(null)
      const result = await backend.flpPreview(nextPath, nextOptions)
      if (!mounted.current) {
        await backend.flpCancel(result.token)
        return
      }
      token.current = result.token
      setPreview(result)
    } catch (e) {
      setError(errorMessage(e))
    } finally {
      if (mounted.current) working(false)
    }
  }
  async function choose() {
    working(true)
    setError(null)
    try {
      const picked = await backend.pickFlpFile()
      if (picked !== null && mounted.current) {
        setPath(picked)
        await review(picked)
      }
    } catch (e) {
      setError(errorMessage(e))
    } finally {
      if (mounted.current) working(false)
    }
  }
  async function folder(
    kind: "factoryDataDir" | "userDataDir" | "sampleSearchFolders"
  ) {
    working(true)
    setError(null)
    try {
      const picked = await backend.pickFolder()
      if (picked === null || !mounted.current) return
      const next = {
        ...options,
        [kind]:
          kind === "sampleSearchFolders"
            ? [...new Set([...options.sampleSearchFolders, picked])]
            : picked,
      }
      setOptions(next)
      if (path) await review(path, next)
    } catch (e) {
      setError(errorMessage(e))
    } finally {
      if (mounted.current) working(false)
    }
  }
  async function removeFolder(folder: string) {
    const next = {
      ...options,
      sampleSearchFolders: options.sampleSearchFolders.filter(
        (p) => p !== folder
      ),
    }
    setOptions(next)
    if (path) await review(path, next)
  }
  async function open() {
    if (!preview || busy) return
    working(true)
    setError(null)
    try {
      if (!(await confirmDiscardChanges()) || !mounted.current) return
      const snapshot = await backend.flpOpen(preview.token)
      token.current = null
      loadSnapshot(snapshot)
      toast.success(t("FL project imported"), {
        description: t("Save as a Windfall project to keep the conversion."),
      })
      onDone()
    } catch (e) {
      setError(errorMessage(e))
    } finally {
      if (mounted.current) working(false)
    }
  }
  return (
    <>
      <FieldGroup>
        <Field>
          <FieldLabel>{t("FL project")}</FieldLabel>
          <Button
            variant="outline"
            onClick={() => void choose()}
            disabled={busy}
          >
            {path ? fileName(path) : t("Choose FL project…")}
          </Button>
          <FieldDescription>
            {t("The current project stays open while you review the import.")}
          </FieldDescription>
        </Field>
        <Collapsible>
          <CollapsibleTrigger
            render={<Button variant="ghost" disabled={busy} />}
          >
            {t("Sample folders (optional)")}
          </CollapsibleTrigger>
          <CollapsibleContent>
            <FieldGroup className="pt-3">
              <Field>
                <FieldLabel>{t("Search folders")}</FieldLabel>
                <FieldDescription>
                  {t("Only selected folders are searched. Ambiguous filenames stay unresolved.")}
                </FieldDescription>
                <Button
                  variant="outline"
                  disabled={busy || options.sampleSearchFolders.length >= 32}
                  onClick={() => void folder("sampleSearchFolders")}
                >
                  {t("Add sample search folder…")}
                </Button>
                {options.sampleSearchFolders.map((folder) => (
                  <div key={folder} className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 wrap-anywhere">
                      {folder}
                    </span>
                    <Button
                      variant="ghost"
                      disabled={busy}
                      aria-label={message("Remove {file}", { file: fileName(folder) })}
                      onClick={() => void removeFolder(folder)}
                    >
                      {t("Remove")}
                    </Button>
                  </div>
                ))}
              </Field>
              <Field>
                <FieldLabel>{t("FL Studio folder")}</FieldLabel>
                <Button
                  variant="outline"
                  disabled={busy}
                  onClick={() => void folder("factoryDataDir")}
                >
                  {options.factoryDataDir ?? t("Choose FL Studio folder…")}
                </Button>
              </Field>
              <Field>
                <FieldLabel>{t("FL Studio user data folder")}</FieldLabel>
                <Button
                  variant="outline"
                  disabled={busy}
                  onClick={() => void folder("userDataDir")}
                >
                  {options.userDataDir ?? t("Choose user data folder…")}
                </Button>
              </Field>
            </FieldGroup>
          </CollapsibleContent>
        </Collapsible>
      </FieldGroup>
      {busy && <p role="status">{t("Preparing import…")}</p>}
      {error && <p role="alert">{error}</p>}
      {preview && <Review preview={preview} />}
      <DialogFooter>
        <Button variant="outline" disabled={busy} onClick={onDone}>
          {t("Cancel")}
        </Button>
        {path && (
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => void review(path)}
          >
            {t("Review again")}
          </Button>
        )}
        <Button disabled={busy || !preview} onClick={() => void open()}>
          {t("Open converted project")}
        </Button>
      </DialogFooter>
    </>
  )
}

/** Review before replacement; cancelling never changes the active project. */
export function FlpImportDialog() {
  const open = useUiStore((state) => state.dialog === "flpImport")
  const [busy, setBusy] = useState(false)
  const close = () => {
    setBusy(false)
    useUiStore.getState().closeDialog()
  }
  return (
    <Dialog
      open={open}
      onOpenChange={(open) => {
        if (!open && !busy) close()
      }}
    >
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("Import FL Studio project")}</DialogTitle>
          <DialogDescription>
            {t("Bring notes and arrangement into Windfall. Missing samples and unsupported sounds are reported; identical playback is not guaranteed.")}
          </DialogDescription>
        </DialogHeader>
        {open && <ImportForm onDone={close} onBusyChange={setBusy} />}
      </DialogContent>
    </Dialog>
  )
}
