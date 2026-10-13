import { AudioWave01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { memo, useState } from "react"

import type { SampleAsset } from "@/bindings"
import { ContextActions, type ContextItem } from "@/components/context-actions"
import { useHint } from "@/lib/store/hint"
import { useProjectStore } from "@/lib/store/project"

import { PICKER_ROW, PickerEmpty, PickerSection } from "../picker-section"
import { matchingPatternIds } from "../pattern-filter"
import { songTick } from "../ops"
import { useAudioClipCounts } from "../selectors"
import { usePlaylistStore } from "../store"
import { placeSampleClips } from "./ops"
import { translatedText as t, message, plainNumber, dynamicTitle } from "@/lib/i18n"

function used(clips: number): string {
  return clips === 0
    ? t("not on the timeline yet")
    : `on the timeline ${clips === 1 ? "once" : message("{clips} times", { clips: plainNumber(clips) })}`
}

/** Puts a clip of the sample at the song position, on a new track. */
async function addAtSongPosition(sample: SampleAsset) {
  const row = useProjectStore.getState().project.playlist.tracks.length
  const created = await placeSampleClips(
    sample,
    [
      {
        row,
        start: Math.max(0, Math.round(songTick())),
        length: 1,
        offset: 0,
        muted: false,
        content: {
          type: "audio",
          sample: sample.id,
          mixerTrack: 0,
          gain: 1,
          pan: 0,
          fadeIn: 0,
          fadeOut: 0,
          reverse: false,
          pitch: 0,
        },
      },
    ],
    "Add audio clip"
  )
  if (created && created.length > 0) usePlaylistStore.getState().select(created)
}

const SampleRow = memo(function SampleRow({
  sample,
  selected,
  clips,
}: {
  sample: SampleAsset
  selected: boolean
  clips: number
}) {
  const setBrush = usePlaylistStore((state) => state.setBrush)
  const hint = useHint(
    selected
      ? message("{sampleName} is what Draw and Paint place, as an audio clip. It is {used}", { sampleName: sample.name, used: used(clips) })
      : message("Click to place {sampleName} as an audio clip with Draw and Paint. It is {used}", { sampleName: sample.name, used: used(clips) })
  )
  const pick = () => setBrush({ type: "audio", sample: sample.id })
  const menu: ContextItem[] = [
    { ...dynamicTitle("Place with Draw and Paint", {}), checked: selected, run: pick },
    {
      ...dynamicTitle("Add to playlist at the song position", {}),
      run: () => addAtSongPosition(sample),
    },
  ]

  return (
    <ContextActions items={menu}>
      <button
        type="button"
        aria-pressed={selected}
        data-sample={sample.id}
        onClick={pick}
        className={PICKER_ROW}
        {...hint}
      >
        <HugeiconsIcon
          icon={AudioWave01Icon}
          strokeWidth={2}
          className="size-3.5 shrink-0 text-muted-foreground"
        />
        <span className="min-w-0 flex-1 truncate group-aria-pressed:font-medium">
          {sample.name}
        </span>
        {clips > 0 && (
          <span className="shrink-0 font-readout text-[0.625rem] text-muted-foreground">
            ×{clips}
          </span>
        )}
      </button>
    </ContextActions>
  )
})

/**
 * The sounds of the project, to place on the timeline as audio clips. One
 * that is picked becomes the brush, as a pattern does.
 */
export function AudioSection() {
  const [query, setQuery] = useState("")
  const samples = useProjectStore((state) => state.project.samples)
  const brush = usePlaylistStore((state) => state.brush)
  const counts = useAudioClipCounts()
  const matchingIds = new Set(matchingPatternIds(samples, query))
  const visibleSamples = samples.filter((sample) => matchingIds.has(sample.id))

  return (
    <PickerSection title={t("Audio")}>
      {samples.length === 0 ? (
        <PickerEmpty>
          {t("Drag a sound from the browser onto the timeline to make an audio clip.")}
        </PickerEmpty>
      ) : (
        <div role="group" aria-label={t("Sound to place")} className="flex flex-col">
          <label className="flex flex-col gap-1 px-2 py-1 text-[0.6875rem] text-muted-foreground">
            {t("Filter sounds")}
            <input
              type="text"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key.startsWith("Arrow")) event.stopPropagation()
              }}
              className="h-7 w-full min-w-0 rounded-md border border-input bg-input/20 px-2 text-xs text-foreground outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30"
            />
          </label>
          {visibleSamples.length === 0 && (
            <PickerEmpty>{t("No sounds match.")}</PickerEmpty>
          )}
          {visibleSamples.map((sample) => (
            <SampleRow
              key={sample.id}
              sample={sample}
              selected={brush.type === "audio" && brush.sample === sample.id}
              clips={counts.get(sample.id) ?? 0}
            />
          ))}
        </div>
      )}
    </PickerSection>
  )
}
