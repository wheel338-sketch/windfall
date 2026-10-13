import { useState, type FormEvent } from "react"

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { usePromptStore } from "@/lib/store/prompts"
import { translatedText as t } from "@/lib/i18n"

/**
 * Remembers the last request, so a dialog still has its words to show while
 * it animates closed after the request is gone.
 */
function useLastRequest<T>(request: T | null): T | null {
  const [last, setLast] = useState(request)
  if (request !== null && request !== last) setLast(request)
  return request ?? last
}

function ConfirmDialog() {
  const request = usePromptStore((state) => state.confirm)
  const shown = useLastRequest(request)

  return (
    <AlertDialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open) request?.resolve(null)
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{shown?.title}</AlertDialogTitle>
          <AlertDialogDescription>{shown?.description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>
            {shown?.cancelLabel ?? t("Cancel")}
          </AlertDialogCancel>
          {shown?.choices.map((choice) => (
            <AlertDialogAction
              key={choice.id}
              variant={choice.variant ?? "default"}
              onClick={() => request?.resolve(choice.id)}
            >
              {choice.label}
            </AlertDialogAction>
          ))}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function TextDialog() {
  const request = usePromptStore((state) => state.text)
  const shown = useLastRequest(request)

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const value = new FormData(event.currentTarget).get("value")
    const text = typeof value === "string" ? value.trim() : ""
    request?.resolve(text === "" ? null : text)
  }

  return (
    <Dialog
      open={request !== null}
      onOpenChange={(open) => {
        if (!open) request?.resolve(null)
      }}
    >
      <DialogContent showCloseButton={false}>
        <form onSubmit={onSubmit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{shown?.title}</DialogTitle>
            {shown?.description && (
              <DialogDescription>{shown.description}</DialogDescription>
            )}
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor="prompt-value">{shown?.label}</FieldLabel>
            <Input
              // A new request must start from its own text, not the last one.
              key={shown?.initial}
              id="prompt-value"
              name="value"
              defaultValue={shown?.initial}
              autoFocus
              autoComplete="off"
              onFocus={(event) => event.target.select()}
            />
          </Field>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => request?.resolve(null)}
            >
              {t("Cancel")}
            </Button>
            <Button type="submit">{shown?.submitLabel}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** Renders the questions asked through `askConfirm` and `askText`. */
export function PromptHost() {
  return (
    <>
      <ConfirmDialog />
      <TextDialog />
    </>
  )
}
