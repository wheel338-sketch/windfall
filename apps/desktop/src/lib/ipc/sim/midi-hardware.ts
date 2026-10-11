import type { ControllerState, MidiHardwareState } from "@/bindings"
import type { Project, Command, DispatchResult } from "@/bindings"
import type { createBrowserScoreLog } from "./score-log"
import type { Backend } from "../backend"

const DESKTOP_ONLY =
  "MIDI hardware is available in the desktop app. Browser simulation has no native MIDI ports."

export function createMidiHardwareMock(context?: {
  project(): Project; generation(): number; dispatch(command: Command): DispatchResult;
  log: ReturnType<typeof createBrowserScoreLog>;
}): Pick<
  Backend,
  | "midiHardwareState"
  | "midiHardwareRefresh"
  | "midiHardwareConfigure"
  | "midiHardwareTarget"
  | "midiHardwarePanic"
  | "controllerState"
  | "controllerSettingsSet"
  | "controllerLearn"
  | "controllerLearnCancel"
  | "noteRecordArm"
  | "automationRecordArm"
  | "scoreLogDump"
  | "scoreLogClear"
> {
  const state = async (): Promise<MidiHardwareState> => ({
    settings: {
      input: null,
      output: null,
      inputChannel: null,
      outputChannel: 1,
      inputProfile: null,
      extraInputs: [],
      sendClock: false,
      followClock: false,
    },
    inputs: [],
    outputs: [],
    inputConnected: false,
    outputConnected: false,
    target: null,
    generation: 0,
    droppedEvents: 0,
    error: DESKTOP_ONLY,
    connectedInputs: [],
    clockRunning: false,
    outputDropped: 0,
    externalBpm: null,
  })
  // Nothing is invented for the browser: no links at work, nothing armed,
  // an empty log, and the reason why.
  const controllers = async (): Promise<ControllerState> => ({
    settings: {
      pickup: false,
      links: [],
      recordOffsetMs: 0,
      recordQuantizeTicks: 0,
    },
    profiles: [],
    learn: null,
    lastInput: null,
    lastTweaked: null,
    waiting: [],
    bank: 0,
    noteRecord: { armed: false, pending: 0, dropped: 0 },
    automationRecord: { armed: false, targets: 0, pending: 0 },
    scoreLog: context ? context.log.state(context.project()) : { notes: 0, seconds: 0 },
    dropped: 0,
    error: DESKTOP_ONLY,
    generation: context?.generation() ?? 0,
  })
  const unavailable = async (): Promise<never> => {
    throw new Error("MIDI hardware requires the desktop app.")
  }
  return {
    midiHardwareState: state,
    midiHardwareRefresh: state,
    midiHardwareConfigure: unavailable,
    midiHardwareTarget: unavailable,
    midiHardwarePanic: async () => {},
    controllerState: controllers,
    controllerSettingsSet: unavailable,
    controllerLearn: unavailable,
    controllerLearnCancel: controllers,
    noteRecordArm: unavailable,
    automationRecordArm: unavailable,
    scoreLogDump: async (pattern, seconds, generation) => {
      if (!context) return unavailable()
      if (generation !== context.generation()) throw new Error("Project changed. Try again.")
      return context.dispatch(context.log.command(context.project(), pattern, seconds, 0))
    },
    scoreLogClear: async () => { context?.log.clear(); return controllers() },
  }
}
