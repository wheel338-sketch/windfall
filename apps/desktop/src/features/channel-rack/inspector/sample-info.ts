import { useEffect, useSyncExternalStore } from "react"

import type { SampleAsset, SampleInfo } from "@/bindings"
import { backend, errorMessage } from "@/lib/ipc"
import { onProjectReplaced } from "@/lib/store/replaced"
import { useWarningsStore } from "@/lib/store/warnings"

export type SampleInfoState =
  | { status: "loading" }
  | { status: "ready"; info: SampleInfo }
  | { status: "error"; message: string }

/*
 * Waveforms are fetched once per sample and kept until the backend reads
 * the sample files anew: when a project loads, and when missing samples are
 * reloaded. An id always means the same file inside one project, but ids
 * start over in the next project, so the file is part of the key.
 */
const cache = new Map<string, SampleInfoState>()
const listeners = new Set<() => void>()
let cacheEpoch = 0

function keyOf(sample: SampleAsset): string {
  return `${sample.id}|${sample.path.kind}|${sample.path.path}`
}

function store(key: string, state: SampleInfoState) {
  cache.set(key, state)
  for (const listener of [...listeners]) listener()
}

const generation = () => useWarningsStore.getState().samplesGeneration

function load(sample: SampleAsset) {
  const key = keyOf(sample)
  if (cache.has(key)) return
  store(key, { status: "loading" })
  const askedAt = generation()
  const askedEpoch = cacheEpoch
  // An answer that comes in after the files were read anew is about what
  // was there before, and the backend refuses a read that another project
  // overtook. Neither says the file is missing, so neither is kept.
  const current = () => generation() === askedAt && cacheEpoch === askedEpoch
  backend.sampleInfoById(sample.id).then(
    (info) => {
      if (current()) store(key, { status: "ready", info })
    },
    (error: unknown) => {
      if (current()) {
        store(key, { status: "error", message: errorMessage(error) })
      }
    }
  )
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** Forgets every waveform. Tests call this between backends. */
export function clearSampleInfo() {
  cacheEpoch++
  cache.clear()
}

// What is on screen asks again once nothing is kept.
function forgetAll() {
  clearSampleInfo()
  for (const listener of [...listeners]) listener()
}

useWarningsStore.subscribe((state, previous) => {
  if (state.samplesGeneration !== previous.samplesGeneration) forgetAll()
})
onProjectReplaced(forgetAll)

/** Facts and waveform of a sample in the project, loaded on first use. */
export function useSampleInfo(
  sample: SampleAsset | undefined
): SampleInfoState | null {
  const key = sample ? keyOf(sample) : null
  const state = useSyncExternalStore(subscribe, () =>
    key === null ? null : (cache.get(key) ?? null)
  )
  // A sample that was forgotten is loaded again here.
  useEffect(() => {
    if (sample && state === null) load(sample)
  }, [sample, state])
  return state
}

/**
 * Whether the sample's file could not be read, which nearly always means
 * it is not where the project expects it. Renders again only when that
 * changes, so a row of the rack can ask without following the waveform.
 */
export function useSampleMissing(sample: SampleAsset | undefined): boolean {
  const key = sample ? keyOf(sample) : null
  const missing = useSyncExternalStore(
    subscribe,
    () => key !== null && cache.get(key)?.status === "error"
  )
  const unread = useSyncExternalStore(
    subscribe,
    () => key !== null && !cache.has(key)
  )
  useEffect(() => {
    if (sample && unread) load(sample)
  }, [sample, unread])
  return missing
}
