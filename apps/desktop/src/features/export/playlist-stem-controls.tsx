import type { ExportPlaylistStems } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { arrangementPlaylist } from "@/features/playlist/arrangement-view"
import { usePlaylistStore } from "@/features/playlist/store"
import { useProjectStore } from "@/lib/store/project"

import { Choice } from "./choice"

const DEFAULT_PLAYLIST_STEMS: ExportPlaylistStems = {
  includeMix: true,
  numbered: true,
  folder: true,
}

/**
 * The choice to export each playlist track to a file of its own. The
 * tracks offered are the ones the active arrangement shows.
 */
export function PlaylistStemControls({
  value,
  disabled,
  onChange,
}: {
  value: ExportPlaylistStems | undefined
  disabled: boolean
  onChange(value: ExportPlaylistStems | undefined): void
}) {
  const playlist = useProjectStore((state) => state.project.playlist)
  const selection = usePlaylistStore((state) => state.selection)
  const shown = arrangementPlaylist(playlist)
  const selectedTracks = shown.tracks
    .filter((track) =>
      shown.clips.some(
        (clip) => clip.track === track.id && selection.has(clip.id)
      )
    )
    .map((track) => track.id)
  const set = <K extends keyof ExportPlaylistStems>(
    key: K,
    next: ExportPlaylistStems[K]
  ) => {
    if (value) onChange({ ...value, [key]: next })
  }
  return (
    <FieldSet disabled={disabled}>
      <FieldLegend variant="label">Playlist tracks</FieldLegend>
      <FieldGroup className="gap-3">
        <Field orientation="horizontal" data-disabled={disabled || undefined}>
          <Checkbox
            id="export-playlist-tracks"
            checked={value !== undefined}
            disabled={disabled}
            onCheckedChange={(checked) =>
              onChange(checked ? DEFAULT_PLAYLIST_STEMS : undefined)
            }
          />
          <FieldLabel htmlFor="export-playlist-tracks">
            Export each playlist track to its own file
          </FieldLabel>
        </Field>
        {value && (
          <>
            <FieldDescription>
              Each file is the song with only that track&apos;s pattern and
              audio clips placed, heard through its mixer tracks, buses and
              the master. Automation plays in every file. Compressors,
              limiters and sidechains react to each track alone, so the files
              add up to the mix only where the path is linear.
            </FieldDescription>
            <Field data-disabled={disabled || undefined}>
              <FieldLabel htmlFor="export-playlist-selection">
                Tracks to export
              </FieldLabel>
              <Choice
                id="export-playlist-selection"
                value={value.tracks === undefined ? "all" : "selected"}
                options={[
                  { value: "all", label: "All tracks that have sound" },
                  { value: "selected", label: "Choose tracks" },
                ]}
                disabled={disabled}
                onChange={(choice) =>
                  set(
                    "tracks",
                    choice === "all"
                      ? undefined
                      : shown.tracks.map((track) => track.id)
                  )
                }
              />
            </Field>
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={disabled || selectedTracks.length === 0}
                onClick={() => set("tracks", selectedTracks)}
              >
                Use tracks of selected clips ({selectedTracks.length})
              </Button>
            </div>
            {value.tracks !== undefined && (
              <FieldSet>
                <FieldLegend variant="label">Selected playlist tracks</FieldLegend>
                <FieldGroup className="max-h-36 gap-2 overflow-y-auto">
                  {shown.tracks.map((track) => (
                    <Field
                      key={track.id}
                      orientation="horizontal"
                      data-disabled={disabled || undefined}
                    >
                      <Checkbox
                        id={`export-playlist-track-${track.id}`}
                        checked={value.tracks?.includes(track.id)}
                        disabled={disabled}
                        onCheckedChange={(checked) =>
                          set(
                            "tracks",
                            checked
                              ? [...(value.tracks ?? []), track.id]
                              : value.tracks?.filter((id) => id !== track.id)
                          )
                        }
                      />
                      <FieldLabel htmlFor={`export-playlist-track-${track.id}`}>
                        {track.name}
                      </FieldLabel>
                    </Field>
                  ))}
                </FieldGroup>
              </FieldSet>
            )}
            <FieldGroup className="gap-2">
              <Field
                orientation="horizontal"
                data-disabled={disabled || undefined}
              >
                <Checkbox
                  id="export-playlist-mix"
                  checked={value.includeMix}
                  disabled={disabled}
                  onCheckedChange={(checked) => set("includeMix", checked)}
                />
                <FieldLabel htmlFor="export-playlist-mix">
                  Include the full mix
                </FieldLabel>
              </Field>
              <Field
                orientation="horizontal"
                data-disabled={disabled || undefined}
              >
                <Checkbox
                  id="export-playlist-numbered"
                  checked={value.numbered}
                  disabled={disabled}
                  onCheckedChange={(checked) => set("numbered", checked)}
                />
                <FieldLabel htmlFor="export-playlist-numbered">
                  Number files in playlist order
                </FieldLabel>
              </Field>
              <Field
                orientation="horizontal"
                data-disabled={disabled || undefined}
              >
                <Checkbox
                  id="export-playlist-folder"
                  checked={value.folder}
                  disabled={disabled}
                  onCheckedChange={(checked) => set("folder", checked)}
                />
                <FieldLabel htmlFor="export-playlist-folder">
                  Put the files in their own folder
                </FieldLabel>
              </Field>
            </FieldGroup>
          </>
        )}
      </FieldGroup>
    </FieldSet>
  )
}
