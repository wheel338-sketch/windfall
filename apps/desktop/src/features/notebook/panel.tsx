import {
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react"

import type { Notebook, NotebookPage } from "@/bindings"
import { PositionInput } from "@/components/position-input"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { actionForEvent, runAction } from "@/lib/actions"
import { subscribeRealtime } from "@/lib/store/realtime"
import { useTransportStore } from "@/lib/store/transport"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"

import { message, useTranslation } from "@/lib/i18n"

import { duplicatePage } from "./duplicate-page"
import { pageAtTick } from "./playback"
import { notebookHtmlUrl } from "./safe-html"

const MAX_PAGES = 8
const TITLE_BYTES = 128
const BODY_BYTES = 16_384
const blankPage = (): NotebookPage => ({ title: "", body: "" })
const byteLength = (value: string) => new TextEncoder().encode(value).length
const normalize = (pages: NotebookPage[]): Notebook => ({
  pages: pages.map((page) => ({ ...page, title: page.title.trim() })),
})
/** Compares books as they are stored: trimmed titles, absent options as off. */
const bookKey = (pages: readonly NotebookPage[]) =>
  JSON.stringify(
    pages.map((page) => [
      page.title.trim(),
      page.body,
      page.startTick ?? null,
      page.html ?? false,
    ])
  )
/** How long typing pauses before the draft goes into the project. */
export const NOTEBOOK_COMMIT_DELAY_MS = 1000
/** Shortcuts that save the project; the draft goes into it first. */
const SAVE_ACTIONS = new Set(["file.save", "file.saveAs", "file.saveNewVersion"])

export function NotebookPanel({
  notebook,
  onSave,
}: {
  notebook?: Notebook
  onSave(notebook: Notebook): Promise<boolean>
}) {
  const t = useTranslation()
  const id = useId()
  const [pages, setPages] = useState(() => notebook?.pages ?? [])
  const [savedKey, setSavedKey] = useState(() => bookKey(notebook?.pages ?? []))
  const [source, setSource] = useState(notebook)
  const [selected, setSelected] = useState(0)
  const [follow, setFollow] = useState(false)
  // `saving` is a Save notebook press, which holds the form. A commit after
  // a pause in typing runs in the background, as `committing`.
  const [saving, setSaving] = useState(false)
  const [committing, setCommitting] = useState<string | null>(null)
  const pending = useRef(false)
  const inFlight = useRef<Promise<void> | null>(null)
  const [error, setError] = useState<string | null>(null)
  // The temporary blank page belongs only to the editor until an edit or add.
  const visiblePages = pages.length ? pages : [blankPage()]
  const page = visiblePages[selected] ?? visiblePages[0]
  const next = normalize(pages)
  const nextKey = bookKey(pages)
  const changed = nextKey !== savedKey
  const cueInvalid = pages.some((item) => item.startTick !== undefined &&
    item.startTick !== null && (!Number.isInteger(item.startTick) ||
    item.startTick < 0 || item.startTick > 960_000_000))
  const titleInvalid = byteLength(page.title.trim()) > TITLE_BYTES
  const bodyInvalid = byteLength(page.body) > BODY_BYTES
  const invalidPage = next.pages.findIndex(
    (item) =>
      byteLength(item.title) > TITLE_BYTES || byteLength(item.body) > BODY_BYTES
  )
  const invalid = invalidPage !== -1 || pages.length > MAX_PAGES || cueInvalid

  // The book changed under the editor: undo, redo or an edit from elsewhere
  // replaces the draft. A commit of this draft coming back changes nothing.
  if (notebook !== source) {
    setSource(notebook)
    const incoming = notebook?.pages ?? []
    const key = bookKey(incoming)
    if (key === committing) {
      setSavedKey(key)
    } else if (key !== savedKey) {
      setPages(incoming)
      setSavedKey(key)
      setSelected((index) => Math.min(index, Math.max(0, incoming.length - 1)))
      setError(null)
    }
  }
  const items = visiblePages.map((item, index) => ({
    value: index,
    label: message("{index}. {title}", { index: index + 1, title: item.title.trim() || t("Untitled page") }),
  }))

  // Only a page transition renders React, not each realtime frame. Keep drafts
  // editable when stopped; playback-follow never dispatches a project edit.
  useEffect(() => {
    if (!follow || saving) return
    let last: number | null = null
    return subscribeRealtime((frame) => {
      if (!frame.playing || useTransportStore.getState().mode !== "song") return
      const index = pageAtTick(pages, frame.tick)
      if (index !== null && index !== last) {
        last = index
        setSelected(index)
      }
    })
  }, [follow, pages, saving])

  const edit = (patch: Partial<NotebookPage>) => {
    setError(null)
    setPages((current) => {
      const base = current.length ? current : [blankPage()]
      return base.map((item, index) =>
        index === selected ? { ...item, ...patch } : item
      )
    })
  }

  // Puts the draft into the project: one undo step, the project marked
  // unsaved, and written with it. A Save notebook press also tidies the
  // titles; a background commit leaves the text being typed alone.
  const commit = (explicit: boolean): Promise<void> => {
    if (pending.current || !changed || invalid) return Promise.resolve()
    pending.current = true
    const book = next
    const key = nextKey
    setCommitting(key)
    if (explicit) setSaving(true)
    setError(null)
    const work = (async () => {
      try {
        if (await onSave(book)) {
          setSavedKey(key)
          if (explicit) setPages(book.pages)
        } else {
          setError(t("Could not save notebook."))
        }
      } catch {
        setError(t("Could not save notebook."))
      } finally {
        pending.current = false
        inFlight.current = null
        setCommitting(null)
        if (explicit) setSaving(false)
      }
    })()
    inFlight.current = work
    return work
  }

  // What the unmount and save-shortcut paths need from the latest render.
  const latest = useRef({ changed, invalid, next, nextKey, committing, onSave, commit })
  useLayoutEffect(() => {
    latest.current = { changed, invalid, next, nextKey, committing, onSave, commit }
  })

  // A pause in typing commits the draft, so closing the notebook or saving
  // the project never leaves notes behind. A failed commit waits for the
  // next edit rather than retrying in a loop.
  const commitInBackground = useEffectEvent(() => void commit(false))
  useEffect(() => {
    if (!changed || invalid || saving || committing !== null || error) return
    const timer = setTimeout(() => commitInBackground(), NOTEBOOK_COMMIT_DELAY_MS)
    return () => clearTimeout(timer)
  }, [nextKey, changed, invalid, saving, committing, error])

  // Closing the notebook commits what the pause has not yet.
  useEffect(
    () => () => {
      const last = latest.current
      if (last.changed && !last.invalid && last.nextKey !== last.committing)
        void last.onSave(last.next).catch(() => {})
    },
    []
  )

  // Saves the project from inside the notebook once the draft is in it.
  const saveProject = async (action: string) => {
    await inFlight.current
    await latest.current.commit(false)
    await runAction(action)
  }

  return (
    <form
      aria-label={t("Song notebook")}
      aria-busy={saving}
      onSubmit={(event) => {
        event.preventDefault()
        void commit(true)
      }}
      onKeyDown={(event) => {
        // The notebook keeps the app's shortcuts from its fields, except
        // the ones that save the project.
        event.stopPropagation()
        const action = actionForEvent(event.nativeEvent)
        if (action && SAVE_ACTIONS.has(action.id)) {
          event.preventDefault()
          if (!event.repeat) void saveProject(action.id)
        }
      }}
    >
      <FieldGroup>
        <Field orientation="horizontal">
          <Checkbox id={`${id}-follow`} checked={follow} onCheckedChange={setFollow} />
          <FieldLabel htmlFor={`${id}-follow`}>{t("Follow song playback")}</FieldLabel>
        </Field>
        <Field data-disabled={saving}>
          <FieldLabel htmlFor={`${id}-page`}>{t("Page")}</FieldLabel>
          <Select
            items={items}
            value={selected}
            disabled={saving}
            onValueChange={(value: number | null) => {
              if (value !== null) setSelected(value)
            }}
          >
            <SelectTrigger id={`${id}-page`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {items.map((item) => (
                  <SelectItem key={item.value} value={item.value}>
                    {item.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <FieldDescription>
            {pages.length
              ? message("{count} of {max} pages", { count: pages.length, max: MAX_PAGES })
              : t("No saved pages. Start with this blank page.")}
          </FieldDescription>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={saving || visiblePages.length >= MAX_PAGES}
              onClick={() => {
                if (visiblePages.length >= MAX_PAGES) return
                setPages([...visiblePages, blankPage()])
                setSelected(visiblePages.length)
              }}
            >
              {t("Add page")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={saving || !pages.length}
              onClick={() => {
                setPages(pages.filter((_, index) => index !== selected))
                setSelected(Math.max(0, selected - 1))
              }}
            >
              {t("Remove page")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={saving || !pages.length || pages.length >= MAX_PAGES}
              onClick={() => {
                const duplicated = duplicatePage(pages, selected, MAX_PAGES)
                if (!duplicated) return
                setPages(duplicated)
                setSelected(selected + 1)
              }}
            >
              {t("Duplicate page")}
            </Button>
          </div>
        </Field>
        <Field data-invalid={cueInvalid}>
          <FieldLabel htmlFor={`${id}-cue`}>{t("Page starts at song position")}</FieldLabel>
          <PositionInput id={`${id}-cue`} optional value={page.startTick ?? undefined}
            disabled={saving} aria-invalid={cueInvalid}
            aria-describedby={`${id}-cue-help`}
            onValueChange={(startTick) => edit({ startTick })} />
          <FieldDescription id={`${id}-cue-help`}>{t("Type bar:beat or bar:beat:tick, such as 9:1. Leave blank for a manual page. Playback, seeks and loops select the latest cue in song mode.")}</FieldDescription>
          {cueInvalid && <FieldError>{t("Use a whole tick from 0 to 960,000,000.")}</FieldError>}
        </Field>
        <Field orientation="horizontal">
          <Checkbox id={`${id}-html`} disabled={saving} checked={page.html ?? false}
            onCheckedChange={(html) => edit({ html: html || undefined })} />
          <FieldLabel htmlFor={`${id}-html`}>{t("Safe HTML preview")}</FieldLabel>
        </Field>
        <Field data-disabled={saving} data-invalid={titleInvalid}>
          <FieldLabel htmlFor={`${id}-title`}>{t("Page title")}</FieldLabel>
          <Input
            id={`${id}-title`}
            value={page.title}
            disabled={saving}
            aria-invalid={titleInvalid}
            aria-describedby={titleInvalid ? `${id}-title-error` : undefined}
            onChange={(event) => edit({ title: event.target.value })}
          />
          {titleInvalid && (
            <FieldError id={`${id}-title-error`}>
              {t("Page title must be at most 128 bytes.")}
            </FieldError>
          )}
        </Field>
        <Field data-disabled={saving} data-invalid={bodyInvalid}>
          <FieldLabel htmlFor={`${id}-body`}>{t("Page body")}</FieldLabel>
          <Textarea
            id={`${id}-body`}
            className="min-h-48"
            value={page.body}
            disabled={saving}
            aria-invalid={bodyInvalid}
            aria-describedby={bodyInvalid ? `${id}-body-error` : undefined}
            onChange={(event) => edit({ body: event.target.value })}
          />
          {bodyInvalid && (
            <FieldError id={`${id}-body-error`}>
              {t("Page body must be at most 16,384 bytes.")}
            </FieldError>
          )}
          <FieldDescription>{t(page.html ? "Basic text formatting and tables. Links are shown as text; scripts, images and remote content are removed." : "Plain text saved with the song.")}</FieldDescription>
        </Field>
        {page.html && <iframe title={t("Notebook HTML preview")} sandbox=""
          referrerPolicy="no-referrer" className="h-64 w-full rounded-md border bg-background"
          src={notebookHtmlUrl(page.body)} />}
        {invalidPage !== -1 && invalidPage !== selected && (
          <FieldError role="alert">
            {message("Page {page} exceeds a text limit.", { page: invalidPage + 1 })}
          </FieldError>
        )}
        {pages.length > MAX_PAGES && (
          <FieldError role="alert">
            {t("A notebook can have at most 8 pages.")}
          </FieldError>
        )}
        {/* Pinned to the bottom of the scrolling popover, so where the notes
            stand is never out of sight. */}
        <div className="sticky bottom-0 z-10 flex flex-col gap-2 border-t bg-popover py-2">
          {error && <FieldError role="alert">{error}</FieldError>}
          <div className="flex items-center gap-2">
            <Button
              type="submit"
              size="sm"
              disabled={saving || !changed || invalid}
            >
              {t(saving ? "Saving…" : "Save notebook")}
            </Button>
            <p role="status" className="text-muted-foreground">
              {!changed
                ? t("Notes are part of the song. Save the project to keep them.")
                : invalid
                  ? t("Fix the marked text to add your changes to the song.")
                  : t("Adding your changes to the song…")}
            </p>
          </div>
        </div>
      </FieldGroup>
    </form>
  )
}
