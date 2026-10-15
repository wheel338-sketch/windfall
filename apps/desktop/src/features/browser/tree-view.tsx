import {
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"

import { ContextActions } from "@/components/context-actions"
import { setSampleDrag } from "@/lib/dnd"
import { useHint } from "@/lib/store/hint"
import { useChannel } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"

import { activateRow, refresh, selectRow } from "./commands"
import { TreeFooter } from "./empty-states"
import { rowMenu } from "./row-menu"
import { useLibraryStore } from "./library-store"
import { captureFile, fileRequestCurrent } from "./file-token"
import {
  collapseFolder,
  expandFolder,
  rememberScrollTop,
  requestDone,
  savedScrollTop,
  toggleFolder,
  useBrowserStore,
} from "./store"
import {
  findByPrefix,
  firstIndex,
  flattenTree,
  flattenLibrary,
  parseRowId,
  isNavigable,
  lastIndex,
  stemOf,
  stepIndex,
  type EntryRow,
  type TreeRow,
} from "./tree-model"
import { TreeRowView } from "./tree-row"
import { ROW_HEIGHT, useVirtualWindow } from "./use-virtual-window"
import { translatedText as t } from "@/lib/i18n"

/** Letters typed closer together than this build up one search. */
const TYPE_AHEAD_MS = 600

/** A small label that follows the pointer while a sound is dragged. */
function showDragImage(event: React.DragEvent, name: string) {
  if (typeof event.dataTransfer.setDragImage !== "function") return
  const label = document.createElement("div")
  label.textContent = name
  label.className =
    "pointer-events-none fixed -top-24 left-0 max-w-56 truncate rounded-md border bg-popover px-2 py-1 text-xs font-medium text-popover-foreground shadow-md"
  document.body.append(label)
  event.dataTransfer.setDragImage(label, 10, 12)
  // The browser has taken its picture by the next task.
  setTimeout(() => label.remove(), 0)
}

/** Gives a row a tooltip with its full text, but only while it is cut off. */
function titleWhenCut(event: React.MouseEvent) {
  if (!(event.target instanceof Element)) return
  const line = event.target.closest<HTMLElement>("[data-index]")
  const label = line?.querySelector<HTMLElement>("[data-name]")
  if (!line || !label) return
  if (line.dataset.libraryPath) {
    line.title = line.dataset.libraryPath
  } else if (label.scrollWidth > label.clientWidth) {
    line.title = label.textContent ?? ""
  } else {
    line.removeAttribute("title")
  }
}

/**
 * The folders and sounds as one flat, windowed list with tree semantics.
 * Selecting a sound plays it, so every way of moving the selection here is a
 * way of auditioning.
 */
export function TreeView() {
  const roots = useBrowserStore((state) => state.roots)
  const listings = useBrowserStore((state) => state.listings)
  const expanded = useBrowserStore((state) => state.expanded)
  const filter = useBrowserStore((state) => state.filter)
  const library = useLibraryStore((state) => state.results)
  const favoritesOnly = useLibraryStore((state) => state.favoritesOnly)
  const tags = useLibraryStore((state) => state.tags)
  const filtering = filter.trim() !== "" || favoritesOnly || tags.length > 0
  const selectedId = useBrowserStore((state) => state.selected?.id ?? null)
  const restoring = useBrowserStore((state) => state.restoring)
  const revealRequest = useBrowserStore((state) => state.reveal)
  const focusRequest = useBrowserStore((state) => state.focusTree)
  const channel = useChannel(useUiStore((state) => state.selectedChannel))

  const tree = useMemo(
    () =>
      filtering
        ? flattenLibrary(library)
        : flattenTree({ roots, listings, expanded, filter: "" }),
    [roots, listings, expanded, filtering, library]
  )
  const { rows, indexOf } = tree
  const selectedIndex =
    selectedId === null ? -1 : (indexOf.get(selectedId) ?? -1)

  const virtual = useVirtualWindow(rows.length)
  const { scrollRef, start, end, pageSize, measure, reveal } = virtual
  const treeRef = useRef<HTMLDivElement>(null)
  const baseId = useId()
  const [menuRowId, setMenuRowId] = useState<string | null>(null)
  const typed = useRef({ text: "", at: 0 })
  const hint = useHint(
    t("Arrow keys move through sounds and play them. Enter adds one to the rack.")
  )

  // Where the list was scrolled to last time. After a restart the open
  // folders come back one by one, so the position is applied again as the
  // list grows, until everything is back or the user scrolls.
  const pendingScroll = useRef<number | null>(savedScrollTop() || null)
  useLayoutEffect(() => {
    const element = scrollRef.current
    const target = pendingScroll.current
    if (!element || target === null) return
    element.scrollTop = target
    measure()
    if (!restoring) pendingScroll.current = null
  }, [rows.length, restoring, scrollRef, measure])

  useEffect(() => {
    if (revealRequest === null) return
    // A row that is not listed yet is scrolled to once it is.
    const index = indexOf.get(revealRequest)
    if (index === undefined) return
    pendingScroll.current = null
    reveal(index)
    requestDone("reveal")
  }, [revealRequest, indexOf, reveal])

  // Selecting a file opens its panes under the tree, and the tree gets
  // shorter. When it does, the selected row is kept in sight, so browsing
  // its neighbours does not start with a scroll to find it again.
  const keepSelectedInSight = useEffectEvent(() => {
    // A position still being restored after a restart wins.
    if (pendingScroll.current !== null || selectedIndex < 0) return
    reveal(selectedIndex)
  })
  useLayoutEffect(() => {
    const element = scrollRef.current
    if (!element) return undefined
    let height = element.clientHeight
    const observer = new ResizeObserver(() => {
      const shrank = element.clientHeight < height
      height = element.clientHeight
      if (shrank) keepSelectedInSight()
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [scrollRef])

  useEffect(() => {
    if (!focusRequest) return
    treeRef.current?.focus()
    requestDone("focusTree")
  }, [focusRequest])

  function rowAt(target: EventTarget | null): TreeRow | null {
    if (!(target instanceof Element)) return null
    // Menu primitives also mark their trigger with data-index. Match actual
    // entries and status rows (including Retry), never the blank-space trigger.
    const line = target.closest<HTMLElement>(
      "[role=treeitem][data-index], [data-status][data-index]"
    )
    if (!line) return null
    return rows[Number(line.dataset.index)] ?? null
  }

  function moveTo(index: number, event: React.KeyboardEvent) {
    event.preventDefault()
    const row = rows[index]
    if (!isNavigable(row)) return
    pendingScroll.current = null
    // A held key flies past sounds; only the one it stops on is played.
    selectRow(row, { preview: true, settle: event.repeat })
    reveal(index)
  }

  function onKeyDown(event: React.KeyboardEvent) {
    // Shortcuts with Ctrl, Cmd or Alt belong to the app's keymap.
    if (event.ctrlKey || event.metaKey || event.altKey) return
    const current: EntryRow | null =
      selectedIndex >= 0 && isNavigable(rows[selectedIndex])
        ? rows[selectedIndex]
        : null

    switch (event.key) {
      case "ArrowDown":
        return moveTo(
          current ? stepIndex(rows, selectedIndex, 1) : firstIndex(rows),
          event
        )
      case "ArrowUp":
        return moveTo(
          current ? stepIndex(rows, selectedIndex, -1) : lastIndex(rows),
          event
        )
      case "PageDown":
        return moveTo(stepIndex(rows, selectedIndex, 1, pageSize), event)
      case "PageUp":
        return moveTo(
          stepIndex(rows, Math.max(selectedIndex, 0), -1, pageSize),
          event
        )
      case "Home":
        return moveTo(firstIndex(rows), event)
      case "End":
        return moveTo(lastIndex(rows), event)
      case "ArrowRight": {
        if (current?.kind !== "folder") return
        event.preventDefault()
        if (!current.open) {
          expandFolder(current.id, current.path)
          return
        }
        const inside = stepIndex(rows, selectedIndex, 1)
        const child = rows[inside]
        if (isNavigable(child) && child.parent === current.id) {
          moveTo(inside, event)
        }
        return
      }
      case "ArrowLeft": {
        if (!current) return
        event.preventDefault()
        if (current.kind === "folder" && current.open && !current.forced) {
          collapseFolder(current.id)
        } else if (current.parent !== null) {
          moveTo(indexOf.get(current.parent) ?? -1, event)
        }
        return
      }
      case "Enter":
        if (!current) return
        event.preventDefault()
        // Holding Enter must not add the same sound over and over.
        if (!event.repeat) activateRow(current)
        return
      default:
    }

    // Space is the app's play and stop key, so it never joins a search.
    if (event.key.length !== 1 || event.key === " ") return
    event.preventDefault()
    const now = performance.now()
    const text =
      now - typed.current.at < TYPE_AHEAD_MS
        ? typed.current.text + event.key
        : event.key
    typed.current = { text, at: now }
    const found = findByPrefix(rows, selectedIndex, text)
    if (found >= 0) moveTo(found, event)
  }

  function onMouseDown(event: React.MouseEvent) {
    if (event.button !== 0) return
    if (
      event.target instanceof Element &&
      event.target.closest("[data-retry]")
    ) {
      return
    }
    pendingScroll.current = null
    const row = rowAt(event.target)
    // Pressing, not releasing, plays the sound: it is heard that much sooner,
    // and a press on the selected sound plays it again.
    if (isNavigable(row)) selectRow(row, { preview: true })
  }

  function onClick(event: React.MouseEvent) {
    const row = rowAt(event.target)
    if (!row) return
    if (row.type === "status") {
      const retry =
        event.target instanceof Element && event.target.closest("[data-retry]")
      if (retry) refresh(row.path)
      return
    }
    if (row.kind === "folder") toggleFolder(row)
  }

  function onDoubleClick(event: React.MouseEvent) {
    const row = rowAt(event.target)
    if (row?.type === "entry" && row.kind !== "folder") activateRow(row)
  }

  function onDragStart(event: React.DragEvent) {
    const row = rowAt(event.target)
    if (row?.type !== "entry" || row.kind !== "audio") {
      event.preventDefault()
      return
    }
    const name = stemOf(row.name, row.kind)
    const file = captureFile(row.path, row.library)
    if (!fileRequestCurrent(file)) {
      event.preventDefault()
      return
    }
    const browser =
      file.browser ??
      (library
        ? {
            path: row.path,
            rootPath: parseRowId(row.id)?.root ?? "",
            generation: library.generation,
            fingerprint: "",
          }
        : undefined)
    if (!browser) {
      event.preventDefault()
      return
    }
    setSampleDrag(event, { path: row.path, name, browser })
    showDragImage(event, name)
  }

  function onContextMenu(event: React.MouseEvent) {
    // The menu key opens the menu for the selected row.
    const row =
      rowAt(event.target) ??
      (event.button !== 2 && selectedIndex >= 0 ? rows[selectedIndex] : null)
    if (row?.type !== "entry") {
      setMenuRowId(null)
      return
    }
    if (row.kind !== "other" && row.id !== selectedId) selectRow(row)
    setMenuRowId(row.id)
  }

  function onScroll() {
    virtual.onScroll()
    // Rows change hands while scrolling, and a tooltip must not go along.
    for (const line of treeRef.current?.querySelectorAll("[title]") ?? []) {
      if (!line.hasAttribute("data-library-path")) line.removeAttribute("title")
    }
    const element = scrollRef.current
    if (element && pendingScroll.current === null) {
      rememberScrollTop(element.scrollTop)
    }
  }

  const menuRow =
    menuRowId === null ? undefined : rows[indexOf.get(menuRowId) ?? -1]
  const menuItems = rowMenu(
    menuRow?.type === "entry" ? menuRow : null,
    channel ?? null
  )

  const lines: ReactNode[] = []
  const pushLine = (index: number, key: string | number) => {
    lines.push(
      <TreeRowView
        key={key}
        row={rows[index]}
        index={index}
        selected={index === selectedIndex}
        domId={`${baseId}-${index}`}
      />
    )
  }
  // Rows are keyed by their slot in the window, not by what they show. A
  // row that scrolls out hands its elements to the one scrolling in, so
  // scrolling changes text and positions and builds almost nothing.
  for (let index = start; index < end; index += 1) {
    pushLine(index, index % virtual.capacity)
  }
  // The selected row stays in the document when it scrolls out of the
  // window, so the tree's active row always exists for screen readers.
  if (selectedIndex >= 0 && (selectedIndex < start || selectedIndex >= end)) {
    pushLine(selectedIndex, "selected")
  }

  return (
    <ContextActions items={menuItems}>
      <div
        ref={scrollRef}
        data-slot="browser-scroll"
        className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto"
        onScroll={onScroll}
        onWheel={() => {
          pendingScroll.current = null
        }}
        onContextMenu={onContextMenu}
        onClick={(event) => {
          // A click below the last row still gives the tree the keyboard.
          if (event.target === event.currentTarget) treeRef.current?.focus()
        }}
      >
        <div
          ref={treeRef}
          role="tree"
          aria-label={t("Sounds and folders")}
          aria-activedescendant={
            selectedIndex >= 0 ? `${baseId}-${selectedIndex}` : undefined
          }
          tabIndex={0}
          // With a row selected, the row shows the focus; without one the
          // tree itself does.
          data-active={selectedIndex >= 0 || undefined}
          className="group/tree relative -outline-offset-2 data-active:outline-none"
          style={{ height: rows.length * ROW_HEIGHT }}
          onKeyDown={onKeyDown}
          onMouseDown={onMouseDown}
          onMouseOver={titleWhenCut}
          onClick={onClick}
          onDoubleClick={onDoubleClick}
          onDragStart={onDragStart}
          {...hint}
        >
          {lines}
        </div>
        <TreeFooter filtering={tree.filtering} rowCount={rows.length} />
      </div>
    </ContextActions>
  )
}
