import type { ChannelId } from "@/bindings"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"

import { holdKey, releaseKey } from "./chords/held-keys"
import { logAuditionOff, logAuditionOn } from "./note-log"
import { translatedText as t } from "@/lib/i18n"

export function auditionOn(channel: ChannelId, key: number, velocity: number) {
  logAuditionOn(channel, key, velocity)
  holdKey(channel, key)
  backend
    .auditionNoteOn(channel, key, velocity)
    .catch((error: unknown) => reportError(error, t("Could not play the note")))
}

export function auditionOff(channel: ChannelId, key: number) {
  logAuditionOff(channel, key)
  releaseKey(channel, key)
  backend
    .auditionNoteOff(channel, key)
    .catch((error: unknown) => reportError(error, t("Could not stop the note")))
}
