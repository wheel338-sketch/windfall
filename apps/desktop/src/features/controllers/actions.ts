import { toast } from "sonner"

import type { ControllerState } from "@/bindings"
import { registry, type Action } from "@/lib/actions"
import { reportError } from "@/lib/errors"
import { backend } from "@/lib/ipc"
import { useTransportStore } from "@/lib/store/transport"

import { learnTargets } from "./menu"

/** Notes older than this are left out of "dump the last two minutes". */
export const RECENT_LOG_SECONDS = 120

async function withState(
  what: string,
  work: (state: ControllerState) => Promise<void>
): Promise<void> {
  try {
    await work(await backend.controllerState())
  } catch (error) {
    reportError(error, what)
  }
}

/** Puts what the note log holds into the pattern that is being edited. */
export function dumpNoteLog(seconds: number | null): Promise<void> {
  return withState("Could not dump the note log", async (state) => {
    const { pattern } = useTransportStore.getState()
    await backend.scoreLogDump(pattern, seconds, state.generation)
    toast.success("Note log dumped to the pattern", {
      description: "Undo takes the notes out again. The log keeps them.",
    })
  })
}

const CONTROLLER_ACTIONS: Action[] = [
  {
    id: "controllers.multiLink",
    title: "Multi-link controllers",
    section: "Tools",
    keywords: "midi learn link several knobs faders hardware in order",
    run: () =>
      void withState("Could not start multi-link", async (state) => {
        if (state.learn?.multi) {
          await backend.controllerLearnCancel()
          toast("Multi-link is off")
          return
        }
        await learnTargets([], true)
      }),
  },
  {
    id: "controllers.linkLastTweaked",
    title: "Link last tweaked control to controller",
    section: "Tools",
    keywords: "midi learn link knob fader hardware remote control",
    run: () =>
      void withState("Could not link the control", async (state) => {
        if (!state.lastTweaked) {
          toast("Move a control on screen first")
          return
        }
        await learnTargets([{ type: "param", target: state.lastTweaked }], false)
      }),
  },
  {
    id: "controllers.cancelLink",
    title: "Stop linking controllers",
    section: "Tools",
    keywords: "midi learn cancel multi-link",
    run: () =>
      void backend
        .controllerLearnCancel()
        .catch((error: unknown) => reportError(error)),
  },
  {
    id: "controllers.togglePickup",
    title: "Toggle controller pickup mode",
    section: "Tools",
    keywords: "takeover hold until hardware reaches value soft",
    run: () =>
      void withState("Could not change pickup mode", async (state) => {
        const next = await backend.controllerSettingsSet({
          ...state.settings,
          pickup: !state.settings.pickup,
        })
        toast(next.settings.pickup ? "Pickup mode is on" : "Pickup mode is off")
      }),
  },
  {
    id: "controllers.toggleNoteRecording",
    title: "Record played notes into the pattern",
    section: "Transport",
    keywords: "midi keyboard record notes piano roll arm",
    run: () =>
      void withState("Could not change note recording", async (state) => {
        const next = await backend.noteRecordArm(!state.noteRecord.armed)
        toast(
          next.noteRecord.armed
            ? "Note recording is armed"
            : "Note recording is off",
          {
            description: next.noteRecord.armed
              ? "Notes played while the transport runs go into the pattern when it stops."
              : undefined,
          }
        )
      }),
  },
  {
    id: "controllers.dumpNoteLog",
    title: "Dump note log to pattern",
    section: "Tools",
    keywords: "score logger played notes missed idea recover",
    run: () => void dumpNoteLog(null),
  },
  {
    id: "controllers.dumpRecentNoteLog",
    title: "Dump last two minutes of the note log to pattern",
    section: "Tools",
    keywords: "score logger played notes missed idea recover recent",
    run: () => void dumpNoteLog(RECENT_LOG_SECONDS),
  },
  {
    id: "controllers.clearNoteLog",
    title: "Clear note log",
    section: "Tools",
    keywords: "score logger forget played notes",
    run: () =>
      void backend
        .scoreLogClear()
        .catch((error: unknown) => reportError(error)),
  },
]

/**
 * The controller commands. They need the desktop app: in a plain browser
 * there is no MIDI hardware, and each reports that when it is run.
 */
export function registerControllerActions(): () => void {
  return registry.register(CONTROLLER_ACTIONS)
}
