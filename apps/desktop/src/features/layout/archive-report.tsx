import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { archiveReport, useArchiveStore } from "@/lib/flows/portable"
import { translatedText as t } from "@/lib/i18n"

/** Persistent, scrollable file report for missing sources and IO failures. */
export function ArchiveReport() {
  const report = useArchiveStore((state) => state.report)
  return (
    <Dialog
      open={report !== null}
      onOpenChange={(open) => {
        if (!open) archiveReport(null)
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Project archive report")}</DialogTitle>
          <DialogDescription>
            {t("Resolve the reported problem, then retry the file action.")}
          </DialogDescription>
        </DialogHeader>
        <Alert variant="destructive">
          <AlertTitle>{t("Archive operation stopped")}</AlertTitle>
          <AlertDescription
            className="max-h-[50vh] overflow-y-auto wrap-anywhere whitespace-pre-wrap"
            tabIndex={0}
          >
            {report}
          </AlertDescription>
        </Alert>
        <DialogFooter>
          <Button onClick={() => archiveReport(null)}>{t("Close report")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
