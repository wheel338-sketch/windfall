import { useId, useMemo, useState } from "react"

import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { attempt } from "@/lib/errors"

import {
  closeSheetExport,
  printSheet,
  saveSheetMusicXml,
  saveSheetPdf,
  useSheetExport,
  type SheetRequest,
} from "../sheet-export"
import { KEY_SIGNATURES } from "./keys"
import { PAPERS, layoutScore, type PaperSize } from "./layout"
import { buildScore, type SmallestValue, type StaffLayout } from "./model"
import { pageSvg } from "./svg"

const PAPER_OPTIONS = (Object.keys(PAPERS) as PaperSize[]).map((value) => ({
  value,
  label: PAPERS[value].label,
}))
const STAFF_OPTIONS: { value: StaffLayout; label: string }[] = [
  { value: "grand", label: "Grand staff" },
  { value: "treble", label: "Treble staff" },
  { value: "bass", label: "Bass staff" },
]
const SMALLEST_OPTIONS: { value: `${SmallestValue}`; label: string }[] = [
  { value: "8", label: "Eighth notes" },
  { value: "16", label: "Sixteenth notes" },
  { value: "32", label: "Thirty-second notes" },
]
const AUTO_KEY = "auto"
const KEY_OPTIONS = [
  { value: AUTO_KEY, label: "Detect from the notes" },
  ...KEY_SIGNATURES.map((key) => ({ value: key.id, label: key.label })),
]

/** A labelled choice from a short list. */
function Choice<Value extends string>({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string
  value: Value
  options: readonly { value: Value; label: string }[]
  onChange(value: Value): void
  disabled?: boolean
}) {
  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Select
        items={options}
        value={value}
        disabled={disabled}
        onValueChange={(next) => {
          const option = options.find((item) => item.value === next)
          if (option) onChange(option.value)
        }}
      >
        <SelectTrigger aria-label={label}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {options.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange(checked: boolean): void
}) {
  const id = useId()
  return (
    <div className="flex items-center justify-between gap-2">
      <label htmlFor={id} className="text-sm">
        {label}
      </label>
      <Switch
        id={id}
        size="sm"
        checked={checked}
        onCheckedChange={(next) => onChange(next)}
      />
    </div>
  )
}

function SheetExportForm({ request }: { request: SheetRequest }) {
  const [paper, setPaper] = useState<PaperSize>("a4")
  const [staff, setStaff] = useState<StaffLayout>("grand")
  const [smallest, setSmallest] = useState<`${SmallestValue}`>("16")
  const [keyId, setKeyId] = useState<string>(AUTO_KEY)
  const [title, setTitle] = useState(true)
  const [tempo, setTempo] = useState(true)
  const [shown, setShown] = useState(0)
  const [pending, setPending] = useState(false)

  const score = useMemo(() => {
    const key = KEY_SIGNATURES.find((item) => item.id === keyId)
    return buildScore(request.pattern, request.channel, {
      smallest: Number(smallest) as SmallestValue,
      staff,
      key: key ? { fifths: key.fifths, mode: key.mode } : "auto",
      baseSignature: request.baseSignature,
    })
  }, [request, smallest, staff, keyId])
  const layout = useMemo(
    () =>
      layoutScore(score, {
        paper,
        title: title ? request.pattern.name : undefined,
        subtitle: title ? request.channelName : undefined,
        composer: title && request.author ? request.author : undefined,
        tempoBpm: tempo ? request.tempoBpm : undefined,
      }),
    [score, paper, title, tempo, request]
  )
  const pages = layout.pages
  const page = Math.min(shown, pages.length - 1)
  const preview = useMemo(() => pageSvg(pages[page]), [pages, page])
  const { notes, quantised, simplified } = score.stats

  const run = async (work: () => Promise<unknown>, what: string) => {
    if (pending) return
    setPending(true)
    try {
      await attempt(work(), what)
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) closeSheetExport()
      }}
    >
      <DialogContent
        className="max-h-[92dvh] overflow-y-auto sm:max-w-4xl"
        showCloseButton={!pending}
      >
        <DialogHeader>
          <DialogTitle>Export sheet music</DialogTitle>
          <DialogDescription>
            {request.pattern.name} · {request.channelName}. Engraved staff
            notation of this channel, ready to print or save.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 md:grid-cols-[14rem_1fr]">
          <FieldGroup className="flex flex-col gap-3">
            <Choice
              label="Paper"
              value={paper}
              options={PAPER_OPTIONS}
              onChange={setPaper}
              disabled={pending}
            />
            <Choice
              label="Staves"
              value={staff}
              options={STAFF_OPTIONS}
              onChange={setStaff}
              disabled={pending}
            />
            <Choice
              label="Key signature"
              value={keyId}
              options={KEY_OPTIONS}
              onChange={setKeyId}
              disabled={pending}
            />
            <Choice
              label="Smallest note value"
              value={smallest}
              options={SMALLEST_OPTIONS}
              onChange={setSmallest}
              disabled={pending}
            />
            <Toggle label="Title and names" checked={title} onChange={setTitle} />
            <Toggle label="Tempo marking" checked={tempo} onChange={setTempo} />
            <p className="text-xs text-muted-foreground" aria-label="Score summary">
              {notes === 1 ? "1 note" : `${notes} notes`} on{" "}
              {pages.length === 1 ? "1 page" : `${pages.length} pages`}
              {score.keyDetected ? ", key detected from the notes" : ""}.
            </p>
            {(quantised > 0 || simplified > 0) && (
              <Alert>
                <AlertDescription>
                  {quantised > 0 &&
                    `${quantised === 1 ? "1 note was" : `${quantised} notes were`} moved to the nearest writable position. `}
                  {simplified > 0 &&
                    `${simplified === 1 ? "1 note was" : `${simplified} notes were`} merged or shortened to fit two voices per staff.`}
                </AlertDescription>
              </Alert>
            )}
          </FieldGroup>
          <div className="flex min-w-0 flex-col gap-2">
            <div
              role="img"
              aria-label={`Page ${page + 1} of ${pages.length}`}
              className="max-h-[60dvh] overflow-auto rounded border bg-muted p-2 [&>svg]:mx-auto [&>svg]:h-auto [&>svg]:w-full [&>svg]:max-w-[40rem] [&>svg]:shadow"
              dangerouslySetInnerHTML={{ __html: preview }}
            />
            <div className="flex items-center justify-center gap-2 text-xs">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={page === 0}
                onClick={() => setShown(page - 1)}
              >
                Previous page
              </Button>
              <span>
                Page {page + 1} of {pages.length}
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={page >= pages.length - 1}
                onClick={() => setShown(page + 1)}
              >
                Next page
              </Button>
            </div>
          </div>
        </div>
        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            disabled={pending}
            onClick={closeSheetExport}
          >
            Close
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={pending}
            onClick={() =>
              run(
                () => saveSheetMusicXml(request),
                "Could not save the MusicXML score"
              )
            }
          >
            Save MusicXML…
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={pending}
            onClick={() =>
              run(
                async () => printSheet(pages, paper),
                "Could not print the score"
              )
            }
          >
            Print…
          </Button>
          <Button
            type="button"
            disabled={pending}
            onClick={() =>
              run(
                () => saveSheetPdf(request, pages),
                "Could not save the PDF score"
              )
            }
          >
            Save PDF…
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Mounted once by the piano roll. Shows while a sheet export is open. */
export function SheetExportDialog() {
  const request = useSheetExport((state) => state.request)
  return request ? (
    <SheetExportForm key={request.serial} request={request} />
  ) : null
}
