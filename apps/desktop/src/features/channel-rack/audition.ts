import { useEffect, useMemo, useRef } from "react"

import type { ChannelId } from "@/bindings"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { DEFAULT_KEY, DEFAULT_VELOCITY } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

export function auditionOn(channel: ChannelId, key: number, velocity: number) {
  backend
    .auditionNoteOn(channel, key, velocity)
    .catch((error: unknown) => reportError(error, t("Could not play the sound")))
}

export function auditionOff(channel: ChannelId, key: number) {
  backend
    .auditionNoteOff(channel, key)
    .catch((error: unknown) => reportError(error, t("Could not stop the sound")))
}

/**
 * Plays a channel for as long as something is held. `stop` is safe to call
 * any number of times, and it is called for you when the window loses focus
 * and when the component goes away, so a note cannot be left sounding.
 */
export function useAudition(channel: ChannelId, key: number = DEFAULT_KEY) {
  const held = useRef(false)

  const audition = useMemo(
    () => ({
      start(velocity: number = DEFAULT_VELOCITY) {
        if (held.current) return
        held.current = true
        auditionOn(channel, key, velocity)
      },
      stop() {
        if (!held.current) return
        held.current = false
        auditionOff(channel, key)
      },
    }),
    [channel, key]
  )

  useEffect(() => {
    window.addEventListener("blur", audition.stop)
    return () => {
      window.removeEventListener("blur", audition.stop)
      audition.stop()
    }
  }, [audition])

  return audition
}
