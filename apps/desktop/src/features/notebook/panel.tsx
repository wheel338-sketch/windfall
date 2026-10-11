import { useEffect, useId, useRef, useState } from "react"

import type { Notebook, NotebookPage } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
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
import { notebookHtmlDocument } from "./safe-html"

const MAX_PAGES = 8
const TITLE_BYTES = 128
const BODY_BYTES = 16_384
const blankPage = (): NotebookPage => ({ title: "", body: "" })
const byteLength = (value: string) => new TextEncoder().encode(value).length
const normalize = (pages: NotebookPage[]): Notebook => ({
  pages: pages.map((page) => ({ ...page, title: page.title.trim() })),
})

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
  const [saved, setSaved] = useState(() => normalize(notebook?.pages ?? []))
  const [selected, setSelected] = useState(0)
  const [follow, setFollow] = useState(false)
  const [saving, setSaving] = useState(false)
  const pending = useRef(false)
  const [error, setError] = useState<string | null>(null)
  // The temporary blank page belongs only to the editor until an edit or add.
  const visiblePages = pages.length ? pages : [blankPage()]
  const page = visiblePages[selected] ?? visiblePages[0]
  const next = normalize(pages)
  const changed = JSON.stringify(next) !== JSON.stringify(saved)
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
  const items = visiblePages.map((item, index) => ({
    value: index,
    label: `${index + 1}. ${item.title.trim() || t("Untitled page")}`,
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
    setPages((current) => {
      const base = current.length ? current : [blankPage()]
      return base.map((item, index) =>
        index === selected ? { ...item, ...patch } : item
      )
    })
  }

  const save = async () => {
    if (pending.current || !changed || invalid) return
    pending.current = true
    setSaving(true)
    setError(null)
    try {
      if (await onSave(next)) {
        setSaved(next)
        setPages(next.pages)
      } else {
        setError(t("Could not save notebook."))
      }
    } catch {
      setError(t("Could not save notebook."))
    } finally {
      pending.current = false
      setSaving(false)
    }
  }

  return (
    <form
      aria-label="Song notebook"
      aria-busy={saving}
      onSubmit={(event) => {
        event.preventDefault()
        void save()
      }}
      onKeyDown={(event) => event.stopPropagation()}
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
              Add page
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
              Remove page
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
              Duplicate page
            </Button>
          </div>
        </Field>
        <Field data-invalid={cueInvalid}>
          <FieldLabel htmlFor={`${id}-cue`}>{t("Page starts at song tick")}</FieldLabel>
          <Input id={`${id}-cue`} type="number" min={0} max={960_000_000}
            step={1} value={page.startTick ?? ""} disabled={saving}
            aria-invalid={cueInvalid}
            onChange={(event) => edit({ startTick: event.target.value === "" ? undefined : Number(event.target.value) })} />
          <FieldDescription>Leave blank for a manual page. Playback, seeks and loops select the latest cue in song mode.</FieldDescription>
          {cueInvalid && <FieldError>Use a whole tick from 0 to 960,000,000.</FieldError>}
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
              Page title must be at most 128 bytes.
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
              Page body must be at most 16,384 bytes.
            </FieldError>
          )}
          <FieldDescription>{page.html ? "Basic text formatting and tables. Links are shown as text; scripts, images and remote content are removed." : "Plain text saved with the song."}</FieldDescription>
        </Field>
        {page.html && <iframe title="Notebook HTML preview" sandbox=""
          referrerPolicy="no-referrer" className="h-64 w-full rounded-md border bg-background"
          srcDoc={notebookHtmlDocument(page.body)} />}
        {invalidPage !== -1 && invalidPage !== selected && (
          <FieldError role="alert">
            Page {invalidPage + 1} exceeds a text limit.
          </FieldError>
        )}
        {pages.length > MAX_PAGES && (
          <FieldError role="alert">
            A notebook can have at most 8 pages.
          </FieldError>
        )}
        {error && <FieldError role="alert">{error}</FieldError>}
        <Button
          type="submit"
          size="sm"
          disabled={saving || !changed || invalid}
        >
          {saving ? "Saving…" : "Save notebook"}
        </Button>
      </FieldGroup>
    </form>
  )
}
