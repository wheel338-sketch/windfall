import { useLayoutEffect, useState, useSyncExternalStore } from "react"

import { scrollToReveal, STRIP_WIDTH } from "./layout"

export type StripView = {
  /** Width of the scrolling area, 0 until it has been measured. */
  width: number
  /** Height the strips get, without the scrollbar. */
  height: number
  /** How far the strips are scrolled, rounded down to a whole strip. */
  offset: number
  /** Height of the horizontal scrollbar, when it takes up room. */
  scrollbar: number
}

const UNMEASURED: StripView = { width: 0, height: 0, offset: 0, scrollbar: 0 }

/**
 * Follows the size and scroll position of the element the strips scroll
 * in. The scroll position is rounded to whole strips, so scrolling renders
 * the panel only when a strip comes into or leaves the view.
 */
function createStripView(initialWidth: number) {
  let width = initialWidth
  let element: HTMLElement | null = null
  let view = UNMEASURED
  const listeners = new Set<() => void>()

  function read() {
    if (!element) return
    const next: StripView = {
      width: element.clientWidth,
      height: element.clientHeight,
      offset: Math.floor(element.scrollLeft / width) * width,
      scrollbar: Math.max(0, element.offsetHeight - element.clientHeight),
    }
    if (
      next.width === view.width &&
      next.height === view.height &&
      next.offset === view.offset &&
      next.scrollbar === view.scrollbar
    ) {
      return
    }
    view = next
    for (const listener of [...listeners]) listener()
  }

  return {
    /** A ref callback for the scrolling element. */
    attach(node: HTMLElement | null) {
      if (!node) return undefined
      element = node
      const observer = new ResizeObserver(read)
      observer.observe(node)
      node.addEventListener("scroll", read, { passive: true })
      read()
      return () => {
        observer.disconnect()
        node.removeEventListener("scroll", read)
        if (element === node) element = null
      }
    },
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    current: () => view,
    /**
     * Takes a new strip width. The scroller stays attached, so the view
     * keeps its measurements instead of starting over unmeasured.
     */
    setWidth(next: number) {
      if (next === width) return
      width = next
      read()
    },
    /** Scrolls just far enough to bring the strip at `index` fully in view. */
    reveal(index: number) {
      if (!element || element.clientWidth === 0) return
      const target = scrollToReveal(
        index,
        element.scrollLeft,
        element.clientWidth,
        width
      )
      if (target === null) return
      element.scrollLeft = target
      read()
    },
  }
}

/** The view of the strip scroller, and the handle to attach and scroll it. */
export function useStripView(width = STRIP_WIDTH) {
  const [store] = useState(() => createStripView(width))
  useLayoutEffect(() => store.setWidth(width), [store, width])
  const view = useSyncExternalStore(store.subscribe, store.current)
  return { view, attach: store.attach, reveal: store.reveal }
}
