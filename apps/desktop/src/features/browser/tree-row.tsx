import {
  Alert02Icon,
  ArrowRight01Icon,
  AudioWave01Icon,
  File01Icon,
  FileMusicIcon,
  Folder01Icon,
  FolderLibraryIcon,
  FolderOpenIcon,
  GuitarIcon,
} from "@hugeicons/core-free-icons"
import { HugeiconsIcon, type IconSvgElement } from "@hugeicons/react"
import { memo, type CSSProperties } from "react"

import { Spinner } from "@/components/ui/spinner"
import { cn } from "@/lib/utils"

import {
  nameSegments,
  type EntryRow,
  type StatusRow,
  type TreeRow,
} from "./tree-model"
import { ROW_HEIGHT } from "./use-virtual-window"
import { translatedText as t } from "@/lib/i18n"

/** How far each level sits to the right of the one above. */
const INDENT = 12

// One line per level, drawn where the arrow of the folder above sits, so the
// lines of stacked rows join up into guides.
const GUIDE_IMAGE =
  "linear-gradient(to right, transparent 6px, var(--wf-grid-line-strong) 6px, var(--wf-grid-line-strong) 7px, transparent 7px)"

function Guides({ depth }: { depth: number }) {
  if (depth === 0) return null
  return (
    <span
      aria-hidden
      data-slot="tree-guides"
      className="h-full shrink-0"
      style={{
        width: depth * INDENT,
        backgroundImage: GUIDE_IMAGE,
        backgroundSize: `${INDENT}px 100%`,
      }}
    />
  )
}

function iconFor(row: EntryRow): IconSvgElement {
  if (row.kind === "audio") return AudioWave01Icon
  if (row.kind === "project") return FileMusicIcon
  if (row.kind === "other") return File01Icon
  if (row.kind === "soundFont") return GuitarIcon
  if (row.root?.kind === "factory") return FolderLibraryIcon
  return row.open ? FolderOpenIcon : Folder01Icon
}

function rowStyle(index: number): CSSProperties {
  return { top: index * ROW_HEIGHT, height: ROW_HEIGHT }
}

function EntryLine({
  row,
  index,
  selected,
  domId,
}: {
  row: EntryRow
  index: number
  selected: boolean
  domId: string
}) {
  const isFolder = row.kind === "folder"
  const inert = row.kind === "other"

  return (
    <div
      id={domId}
      role="treeitem"
      aria-level={row.depth + 1}
      aria-posinset={row.position}
      aria-setsize={row.setSize}
      aria-selected={selected}
      aria-description={row.relativePath}
      title={row.relativePath ? row.path : undefined}
      data-library-path={row.relativePath ? row.path : undefined}
      aria-expanded={isFolder ? row.open : undefined}
      aria-busy={row.busy || undefined}
      aria-disabled={inert || undefined}
      data-index={index}
      data-kind={row.kind}
      draggable={row.kind === "audio"}
      style={rowStyle(index)}
      className={cn(
        "absolute inset-x-0 flex items-center pr-2 pl-1.5 whitespace-nowrap",
        inert ? "text-muted-foreground/55" : "hover:bg-accent/50",
        row.depth === 0 && "font-medium",
        selected &&
          "bg-accent group-focus/tree:bg-brand/20 group-focus/tree:shadow-[inset_2px_0_0_var(--wf-brand)] group-focus-visible/tree:outline-1 group-focus-visible/tree:-outline-offset-1 group-focus-visible/tree:outline-brand/70 hover:bg-accent"
      )}
    >
      <Guides depth={row.depth} />
      <span
        aria-hidden
        className="mr-0.5 flex size-3.5 shrink-0 items-center justify-center text-muted-foreground"
      >
        {isFolder && (
          <HugeiconsIcon
            icon={ArrowRight01Icon}
            strokeWidth={2}
            className={cn("size-3", row.open && "rotate-90")}
          />
        )}
      </span>
      <span
        aria-hidden
        className={cn(
          "mr-1.5 flex size-3.5 shrink-0 items-center justify-center",
          row.kind === "audio" ? "text-brand" : "text-muted-foreground",
          inert && "text-muted-foreground/55",
          row.failed && "text-destructive"
        )}
      >
        {row.busy ? (
          <Spinner className="size-3" />
        ) : (
          <HugeiconsIcon
            icon={row.failed ? Alert02Icon : iconFor(row)}
            strokeWidth={2}
            className="size-3.5"
          />
        )}
      </span>
      <span data-name className="min-w-0 truncate">
        {nameSegments(row.name, row.kind, row.match).map((segment, part) =>
          segment.marked ? (
            <mark
              key={part}
              className="rounded-[2px] bg-brand/40 text-foreground"
            >
              {segment.text}
            </mark>
          ) : (
            <span
              key={part}
              className={
                segment.extension ? "text-muted-foreground/70" : undefined
              }
            >
              {segment.text}
            </span>
          )
        )}
      </span>
      {row.relativePath && (
        <span
          aria-hidden
          className="ml-2 min-w-0 flex-1 truncate text-muted-foreground"
        >
          {row.relativePath}
        </span>
      )}
    </div>
  )
}

function StatusLine({ row, index }: { row: StatusRow; index: number }) {
  return (
    <div
      role="none"
      data-index={index}
      data-status={row.status}
      style={rowStyle(index)}
      className="absolute inset-x-0 flex items-center pr-2 pl-1.5 whitespace-nowrap text-muted-foreground"
    >
      <Guides depth={row.depth} />
      <span aria-hidden className="mr-0.5 size-3.5 shrink-0" />
      {row.status === "loading" && (
        <>
          <Spinner className="mr-1.5 size-3 shrink-0" />
          <span className="truncate">{t("Reading…")}</span>
        </>
      )}
      {row.status === "empty" && (
        <span className="truncate italic">{t("Empty folder")}</span>
      )}
      {row.status === "error" && (
        <>
          {/* The folder's warning icon says it failed; the room here goes
              to the reason. */}
          <span data-name role="alert" className="min-w-0 truncate">
            <span className="sr-only">{t("Could not read this folder.")}{" "}</span>
            {row.message || t("Could not read this folder.")}
          </span>
          <button
            type="button"
            data-retry
            // Enter on the folder row retries, so this stays out of the tab order.
            tabIndex={-1}
            className="ml-auto shrink-0 rounded-sm pl-2 font-medium text-foreground underline underline-offset-2 hover:text-brand"
          >
            {t("Retry")}
          </button>
        </>
      )}
    </div>
  )
}

type TreeRowViewProps = {
  row: TreeRow
  index: number
  selected: boolean
  domId: string
}

/** One line of the tree. Pointer and key handling sit on the tree itself. */
export const TreeRowView = memo(function TreeRowView({
  row,
  index,
  selected,
  domId,
}: TreeRowViewProps) {
  if (row.type === "status") return <StatusLine row={row} index={index} />
  return <EntryLine row={row} index={index} selected={selected} domId={domId} />
})
