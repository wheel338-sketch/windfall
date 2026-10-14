import { screen } from "@testing-library/react"

import type { Channel, Project } from "@/bindings"
import {
  isInstrumentChannel,
  isSamplerChannel,
  type InstrumentChannel,
  type SamplerSource,
} from "@/lib/channel-source"
import { useProjectStore } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { selectedPatternId } from "@/lib/store/selectors"
import { useTransportStore } from "@/lib/store/transport"
import { settle, startTestApp } from "@/test/harness"

import { registerChannelRackActions } from "./actions"
import { clearSampleInfo } from "./inspector/sample-info"
import { useRackStore } from "./rack-store"
import { litSteps } from "./steps"
import { message, translatedText as t, historyLabelText } from "@/lib/i18n"

/** The test app with the rack's actions registered and its own state reset. */
export async function startRack(options: { project?: Project } = {}) {
  const app = await startTestApp(options)
  const unregister = registerChannelRackActions()
  clearSampleInfo()
  useRackStore.setState(useRackStore.getInitialState(), true)
  return {
    backend: app.backend,
    stop() {
      unregister()
      app.stop()
    },
  }
}

export const project = () => useProjectStore.getState().project
export const history = () => useProjectStore.getState().history
/** The history as it is read: a label is kept as a key and made into words when shown. */
export const labels = () => history().entries.map((entry) => historyLabelText(entry.label))

export function channel(name: string): Channel {
  const found = project().channels.find((item) => item.name === name)
  if (!found) throw new Error(message("No channel called {name}", { name }))
  return found
}

/** The sampler settings of a channel, which must be a sampler. */
export function samplerOf(name: string): SamplerSource {
  const found = channel(name)
  if (!isSamplerChannel(found)) throw new Error(message("{name} is not a sampler", { name }))
  return found.source
}

/** A channel that must be an instrument. */
export function instrumentChannel(name: string): InstrumentChannel {
  const found = channel(name)
  if (!isInstrumentChannel(found)) {
    throw new Error(message("{name} is not an instrument", { name }))
  }
  return found
}

export const channelNames = () => project().channels.map((item) => item.name)

function currentPattern() {
  const id = selectedPatternId(project(), useTransportStore.getState().pattern)
  const pattern = project().patterns.find((item) => item.id === id)
  if (!pattern) throw new Error(t("No pattern"))
  return pattern
}

export function notesOf(name: string) {
  const id = channel(name).id
  return currentPattern().lanes.find((lane) => lane.channel === id)?.notes ?? []
}

const draw = (steps: boolean[]) =>
  steps.map((lit) => (lit ? "x" : ".")).join("")

/** The channel's steps in the store, as "x..." text. */
export function storedRow(name: string): string {
  return draw(litSteps(notesOf(name), currentPattern().lengthSteps))
}

export const stepGrid = (name: string) =>
  screen.getByRole("group", { name: `${name} steps` })

export const stepButtons = (name: string) => [
  ...stepGrid(name).querySelectorAll<HTMLElement>("[data-step]"),
]

/** The channel's steps on screen, as "x..." text. */
export function shownRow(name: string): string {
  return draw(
    stepButtons(name).map(
      (step) => step.getAttribute("aria-pressed") === "true"
    )
  )
}

/** Gives a step grid a width, which jsdom has no layout to provide. */
export function layoutGrid(grid: HTMLElement, width: number) {
  grid.getBoundingClientRect = () =>
    ({
      left: 0,
      top: 0,
      width,
      height: 22,
      right: width,
      bottom: 22,
    }) as DOMRect
}

/** Types into the rename dialog, which the tests do not render. */
export async function answerText(text: string | null) {
  await settle()
  usePromptStore.getState().text?.resolve(text)
  await settle()
}

export async function answerConfirm(choice: string | null) {
  await settle()
  usePromptStore.getState().confirm?.resolve(choice)
  await settle()
}

/** A drag event's data, as the browser panel and the rack fill it in. */
export function dragData(type: string, value: string) {
  return {
    types: [type],
    getData: (asked: string) => (asked === type ? value : ""),
    setData: () => undefined,
    setDragImage: () => undefined,
    dropEffect: "none",
    effectAllowed: "all",
  }
}
