import { logicalDelta } from "@/lib/ui-scale"
import { useEffect, useRef, useState, type Ref } from "react"

import type { ChannelId } from "@/bindings"
import {
  noteName,
  PianoKeyboard,
  type PianoKeyboardHandle,
} from "@/components/audio"
import {
  ContextActions,
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { useHint } from "@/lib/store/hint"

import { auditionOff, auditionOn } from "./audition"
import { useSession } from "./context"
import { HOME_KEY, MAX_KEY } from "./edit-math"
import { handleWheel } from "./grid-input"
import { VIEW_MENU } from "./menu"
import { ROW_COUNT } from "./view-math"
import { translatedText as t, dynamicTitle } from "@/lib/i18n"

/** Below this row height the C labels would run into each other. */
const MIN_LABEL_ROW_PX = 9

type KeyGutterProps = {
  channelId: ChannelId
  /** The channel's color, for keys that are held or lit. */
  color: string
  /** Receives the keyboard so keys can be lit without rendering. */
  keyboardRef: Ref<PianoKeyboardHandle>
}

/**
 * The keyboard down the left side. It is as tall as all 128 rows and slides
 * behind its window as the grid scrolls, so every key sits beside its row.
 */
export function KeyGutter({ channelId, color, keyboardRef }: KeyGutterProps) {
  const session = useSession()
  const windowRef = useRef<HTMLDivElement>(null)
  const innerRef = useRef<HTMLDivElement>(null)
  const [labels, setLabels] = useState(true)
  const [menuKey, setMenuKey] = useState(HOME_KEY)
  const hint = useHint(
    t("Click or drag across the keys to hear them. Right-click a key to select its notes")
  )

  const name = noteName(menuKey)
  const entries: ContextItem[] = [
    { label: name },
    {
      ...dynamicTitle("Select the notes on {name}", { name }),
      run: () => session.editor.selectKey(menuKey, false),
    },
    {
      ...dynamicTitle("Add the notes on {name} to the selection", { name }),
      run: () => session.editor.selectKey(menuKey, true),
    },
    contextSeparator,
    ...VIEW_MENU,
  ]

  useEffect(() => {
    const sync = () => {
      const view = session.view
      const inner = innerRef.current
      if (!view || !inner) return
      const { rowHeight, scrollRow, dpr } = view.viewport
      // The same rounding as the canvas, so keys and rows share pixels.
      const offset = Math.round(scrollRow * rowHeight * dpr) / dpr
      inner.style.height = `${ROW_COUNT * rowHeight}px`
      inner.style.transform = `translateY(${-offset}px)`
      setLabels(rowHeight >= MIN_LABEL_ROW_PX)
    }
    sync()
    return session.onView(sync)
  }, [session])

  useEffect(() => {
    const element = windowRef.current
    if (!element) return
    const onWheel = (event: WheelEvent) => {
      const bounds = element.getBoundingClientRect()
      handleWheel(
        session,
        event,
        { x: 0, y: logicalDelta(event.clientY - bounds.top) },
        { time: false, rows: true }
      )
    }
    element.addEventListener("wheel", onWheel, { passive: false })
    return () => element.removeEventListener("wheel", onWheel)
  }, [session])

  return (
    <ContextActions items={entries}>
      <div
        ref={windowRef}
        className="relative h-full w-full overflow-hidden bg-(--wf-key-black,oklch(0.2_0_0))"
        onContextMenuCapture={(event) => {
          const key = (event.target as HTMLElement).closest<HTMLElement>(
            "[data-key]"
          )?.dataset.key
          if (key !== undefined) setMenuKey(Number(key))
        }}
        {...hint}
      >
        <div
          ref={innerRef}
          className="absolute inset-x-0 top-0 will-change-transform"
        >
          <PianoKeyboard
            ref={keyboardRef}
            orientation="vertical"
            layout="uniform"
            lowKey={0}
            highKey={MAX_KEY}
            showLabels={labels}
            color={color}
            className="h-full w-full rounded-none shadow-none"
            onNoteOn={(key, velocity) => auditionOn(channelId, key, velocity)}
            onNoteOff={(key) => auditionOff(channelId, key)}
            // The keyboard blocks the browser's menu; ours opens instead.
            onContextMenu={undefined}
          />
        </div>
      </div>
    </ContextActions>
  )
}
