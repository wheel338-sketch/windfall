import { useEffect, type ReactNode, type WheelEvent } from "react"
import type { MixerDock, TrackId } from "@/bindings"
import { logicalWheel } from "@/lib/ui-scale"
import { ADD_WIDTH, visibleRange, type StripLayout } from "./layout"
import { MixerStrip } from "./strip"
import { useStripView } from "./strip-view"
import { message, translatedText as t } from "@/lib/i18n"

/** Each dock retains its own scroll position and virtualized strip window. */
export function DockStrips({
  dock,
  ids,
  selected,
  linked,
  layout,
  onHeight,
  onScrollbar,
  scrollbarSpace = 0,
  children,
}: {
  dock: MixerDock
  ids: TrackId[]
  selected: TrackId | null
  linked: TrackId | null
  layout: StripLayout & { width: number }
  onHeight?: (height: number) => void
  onScrollbar?: (height: number) => void
  scrollbarSpace?: number
  children?: ReactNode
}) {
  const { view, attach, reveal } = useStripView(layout.width)
  const range = visibleRange(
    view.offset,
    (view.width || 1280) + layout.width,
    ids.length,
    layout.width
  )
  const selectedIndex = selected === null ? -1 : ids.indexOf(selected)
  const linkedIndex = linked === null ? -1 : ids.indexOf(linked)
  const mounted = Array.from(
    { length: range.end - range.start },
    (_, index) => index + range.start
  )
  if (selectedIndex >= 0 && !mounted.includes(selectedIndex))
    mounted.push(selectedIndex)
  useEffect(() => {
    onHeight?.(view.height)
  }, [view.height, onHeight])
  useEffect(() => {
    onScrollbar?.(view.scrollbar)
  }, [view.scrollbar, onScrollbar])
  useEffect(() => {
    if (selectedIndex >= 0) reveal(selectedIndex)
  }, [selectedIndex, selected, reveal])
  useEffect(() => {
    if (linkedIndex >= 0) reveal(linkedIndex)
  }, [linkedIndex, linked, reveal])
  function onWheel(event: WheelEvent<HTMLDivElement>) {
    if (
      event.defaultPrevented ||
      event.ctrlKey ||
      Math.abs(event.deltaY) <= Math.abs(event.deltaX)
    )
      return
    const target = event.target
    if (!(target instanceof Element)) return
    const list = target.closest("ul")
    if (list && list.scrollHeight > list.clientHeight) return
    event.currentTarget.scrollLeft += logicalWheel(event, {
      line: 1,
      page: 1,
    }).deltaY
  }
  return (
    <div
      ref={attach}
      role="group"
      aria-label={message("{dock} mixer dock", { dock: t(dock) })}
      data-slot={`mixer-dock-${dock}`}
      className={
        dock === "middle"
          ? "min-w-0 flex-1 overflow-x-auto overflow-y-hidden"
          : "z-10 min-w-0 shrink-0 overflow-x-auto overflow-y-hidden border-x bg-chassis"
      }
      style={
        dock === "middle"
          ? undefined
          : { width: Math.min(ids.length * layout.width, 280), maxWidth: "25%" }
      }
      onWheel={onWheel}
    >
      <div
        className="relative h-full"
        style={{
          width: ids.length * layout.width + (children ? ADD_WIDTH : 0),
          minWidth: "100%",
          height:
            scrollbarSpace > view.scrollbar
              ? `calc(100% - ${scrollbarSpace - view.scrollbar}px)`
              : undefined,
        }}
      >
        {mounted.map((index) => (
          <div
            key={ids[index]}
            className="absolute inset-y-0"
            style={{ left: index * layout.width, width: layout.width }}
          >
            <MixerStrip
              id={ids[index]}
              mode={layout.mode}
              sendRows={layout.sendRows}
              effectRows={layout.effectRows}
              linked={ids[index] === linked}
              metering={index >= range.start && index < range.end}
            />
          </div>
        ))}
        {children && (
          <div
            className="absolute inset-y-0"
            style={{ left: ids.length * layout.width }}
          >
            {children}
          </div>
        )}
      </div>
    </div>
  )
}
