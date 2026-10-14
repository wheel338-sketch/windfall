import { useId, useRef, useState } from "react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { editArrangementBook, parseReferences } from "./model"
import type {
  Arrangement,
  ArrangementBook,
  ArrangementEdit,
  NamedReference,
  TrackKind,
} from "./model"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export interface ArrangementPanelProps {
  value: ArrangementBook
  onChange?: (book: ArrangementBook) => void
  /** Project mode: the backend allocates IDs and publishes the saved value. */
  onEdit?: (edit: ArrangementEdit) => Promise<void>
  onMakeUnique?: (clip: number) => Promise<void>
  makeUniqueClipIds?: number[]
  /**
   * Project mode: adds a timeline layout of its own and switches to it.
   * With `copy` the active layout's clips are cloned as new clips;
   * without it the layout starts with the same tracks and no clips.
   */
  onAddLayout?: (name: string, copy: boolean) => Promise<void>
  clips?: NamedReference[]
  tracks: NamedReference[]
  clipIds: number[]
  channels: NamedReference[]
  sources: NamedReference[]
  /** Optional caller allocator. Without one, IDs are monotonic for this panel's lifetime. */
  allocateId?: () => number
}

function Choice({
  label,
  value,
  items,
  onChange,
}: {
  label: string
  value: string
  items: { value: string; label: string }[]
  onChange(value: string): void
}) {
  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Select
        items={items}
        value={value}
        onValueChange={(value: string | null) => {
          if (value !== null) onChange(value)
        }}
      >
        <SelectTrigger aria-label={label}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {items.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

function NameEditor({
  label,
  initial = "",
  action,
  onSubmit,
}: {
  label: string
  initial?: string
  action: string
  onSubmit(name: string): void
}) {
  const id = useId()
  const [name, setName] = useState(initial)
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        value={name}
        maxLength={256}
        onChange={(event) => setName(event.target.value)}
      />
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => onSubmit(name)}
      >
        {action}
      </Button>
    </Field>
  )
}

/** A name and the two ways to start a timeline layout of its own. */
function LayoutEditor({
  suggested,
  onSubmit,
}: {
  suggested: string
  onSubmit(name: string, copy: boolean): void
}) {
  const id = useId()
  const [name, setName] = useState(suggested)
  return (
    <Field>
      <FieldLabel htmlFor={id}>{t("New layout name")}</FieldLabel>
      <Input
        id={id}
        value={name}
        maxLength={256}
        onChange={(event) => setName(event.target.value)}
      />
      <div className="flex flex-wrap gap-1">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onSubmit(name, false)}
        >
          {t("New empty layout")}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onSubmit(name, true)}
        >
          {t("Duplicate layout")}
        </Button>
      </div>
      <FieldDescription>
        {t("A layout is a timeline of its own. Clips placed, moved or deleted in one layout leave the others as they are. Duplicate copies the clips of the layout shown now; the first new layout keeps the present timeline as Arrangement 1.")}
      </FieldDescription>
    </Field>
  )
}

function ReferencesEditor({
  item,
  tracks,
  clipIds,
  clips: namedClips,
  apply,
}: {
  item: Arrangement
  tracks: NamedReference[]
  clipIds: number[]
  clips: NamedReference[]
  apply(work: () => ArrangementEdit): void
}) {
  const id = useId()
  const [clips, setClips] = useState(item.clips.join(", "))
  const [trackIds, setTracks] = useState(item.tracks.join(", "))
  return (
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor={`${id}-clips`}>{t("Ordered clip IDs")}</FieldLabel>
        <Input
          id={`${id}-clips`}
          value={clips}
          onChange={(event) => setClips(event.target.value)}
        />
        <FieldDescription>
          {t("Comma-separated IDs in arrangement order. Available:")}{" "}
          {namedClips.map((clip) => `${clip.id} (${clip.name})`).join(", ") ||
            t("none")}
          .
        </FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor={`${id}-tracks`}>{t("Ordered track IDs")}</FieldLabel>
        <Input
          id={`${id}-tracks`}
          value={trackIds}
          onChange={(event) => setTracks(event.target.value)}
        />
        <FieldDescription>
          {t("Available:")}{" "}
          {tracks.map((track) => `${track.id} (${track.name})`).join(", ") ||
            t("none")}
          .
        </FieldDescription>
      </Field>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() =>
          apply(() => ({
            type: "setReferences",
            id: item.id,
            clips: parseReferences(clips, clipIds),
            tracks: parseReferences(
              trackIds,
              tracks.map((track) => track.id)
            ),
          }))
        }
      >
        {t("Update arrangement references")}
      </Button>
    </FieldGroup>
  )
}

/** Controlled editor: project mode dispatches commands and waits for saved patches. */
export function ArrangementPanel({
  value,
  onChange,
  onEdit,
  onMakeUnique,
  makeUniqueClipIds,
  onAddLayout,
  clips: namedClips,
  tracks,
  clipIds,
  channels,
  sources,
  allocateId,
}: ArrangementPanelProps) {
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  const clips = namedClips ?? clipIds.map((id) => ({ id, name: `Clip ${id}` }))
  const [groupClips, setGroupClips] = useState("")
  const clipInput = useId()
  const lastId = useRef(0)
  const freshId = () => {
    const allocated =
      allocateId?.() ??
      Math.max(
        lastId.current,
        0,
        ...value.arrangements.map((item) => item.id),
        ...value.trackGroups.map((item) => item.id),
        ...value.clipGroups.map((item) => item.id),
        ...tracks.map((item) => item.id),
        ...clipIds,
        ...channels.map((item) => item.id),
        ...sources.map((item) => item.id)
      ) + 1
    if (
      !Number.isInteger(allocated) ||
      allocated <= 0 ||
      allocated > 0xffffffff
    )
      throw new Error(t("No unsigned 32-bit IDs remain."))
    lastId.current = allocated
    return allocated
  }
  const work = async (action: () => void | Promise<void>) => {
    if (pending.current) return
    pending.current = true
    setBusy(true)
    try {
      await action()
      setError(null)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    } finally {
      pending.current = false
      setBusy(false)
    }
  }
  const apply = (edit: () => ArrangementEdit) => {
    if (onEdit) {
      void work(() => onEdit(edit()))
      return
    }
    try {
      const next = editArrangementBook(value, edit(), (kind) =>
        kind.type === "instrument"
          ? channels.some((item) => item.id === kind.channel)
          : kind.type === "audio"
            ? sources.some((item) => item.id === kind.source)
            : // Mixer tracks are not listed here; the project checks them.
              true
      )
      onChange?.(next)
      setError(null)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    }
  }
  const active = value.arrangements.find((item) => item.id === value.active)
  const groupOptions = [
    { value: "none", label: t("Ungrouped") },
    ...value.trackGroups.map((group) => ({
      value: String(group.id),
      label: group.name,
    })),
  ]
  const linkOptions = [
    { value: "none", label: t("Unlinked") },
    ...channels.map((channel) => ({
      value: `instrument:${channel.id}`,
      label: message("Instrument: {channelName}", { channelName: channel.name }),
    })),
    ...sources.map((source) => ({
      value: `audio:${source.id}`,
      label: message("Audio: {sourceName}", { sourceName: source.name }),
    })),
  ]
  const linkValue = (kind: TrackKind | undefined) =>
    kind
      ? `${kind.type}:${
          kind.type === "instrument"
            ? kind.channel
            : kind.type === "audio"
              ? kind.source
              : kind.mixer
        }`
      : "none"
  // A track linked to a mixer track from its header keeps that choice
  // listed here, so the select shows what is saved.
  const mixerLinks = Object.values(value.linkedTracks).flatMap((kind) =>
    kind?.type === "audioTrack"
      ? [
          {
            value: `audioTrack:${kind.mixer}`,
            label: message("Audio track: mixer track {mixer}", { mixer: plainNumber(kind.mixer) }),
          },
        ]
      : []
  )
  const linkChoices = [
    ...linkOptions,
    ...mixerLinks.filter(
      (item, index) =>
        mixerLinks.findIndex((other) => other.value === item.value) === index
    ),
  ]
  return (
    <section
      aria-label={t("Playlist arrangements")}
      className="flex flex-col gap-4 p-3"
    >
      <Alert>
        <AlertDescription>
          {onEdit
            ? t("Arrangement edits are saved with the project. The playlist shows, plays and exports the active arrangement.")
            : t("Local arrangement edits only. Persistence and playlist playback are not connected.")}
        </AlertDescription>
      </Alert>
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <fieldset disabled={busy} className="min-w-0">
        <FieldGroup>
          <FieldSet>
            <FieldLegend>{t("Alternative arrangements")}</FieldLegend>
            {active && (
              <Choice
                label={t("Active arrangement")}
                value={String(active.id)}
                items={value.arrangements.map((item) => ({
                  value: String(item.id),
                  label: item.name,
                }))}
                onChange={(id) =>
                  apply(() => ({ type: "switchArrangement", id: Number(id) }))
                }
              />
            )}
            {onAddLayout && (
              <LayoutEditor
                key={value.arrangements.length}
                suggested={`Arrangement ${Math.max(2, value.arrangements.length + 1)}`}
                onSubmit={(name, copy) => {
                  void work(() => onAddLayout(name, copy))
                }}
              />
            )}
            <NameEditor
              label={t("New arrangement name")}
              action={t("Add arrangement")}
              onSubmit={(name) =>
                apply(() => ({
                  type: "addArrangement",
                  arrangement: {
                    id: freshId(),
                    name,
                    clips: active ? [...active.clips] : [...clipIds],
                    tracks: active
                      ? [...active.tracks]
                      : tracks.map((track) => track.id),
                  },
                }))
              }
            />
            {active && (
              <>
                <NameEditor
                  key={`${active.id}:${active.name}`}
                  label={t("Arrangement name")}
                  initial={active.name}
                  action={t("Rename arrangement")}
                  onSubmit={(name) =>
                    apply(() => ({
                      type: "renameArrangement",
                      id: active.id,
                      name,
                    }))
                  }
                />
                <ReferencesEditor
                  key={`${active.id}:${active.clips.join(",")}:${active.tracks.join(",")}`}
                  item={active}
                  tracks={tracks}
                  clipIds={clipIds}
                  clips={clips}
                  apply={apply}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={value.arrangements.length <= 1}
                  onClick={() =>
                    apply(() => ({ type: "removeArrangement", id: active.id }))
                  }
                >
                  {t("Delete arrangement")}
                </Button>
              </>
            )}
          </FieldSet>
          <FieldSet>
            <FieldLegend>{t("Track groups")}</FieldLegend>
            <NameEditor
              label={t("New track group name")}
              action={t("Add track group")}
              onSubmit={(name) =>
                apply(() => ({
                  type: "addTrackGroup",
                  group: { id: freshId(), name },
                }))
              }
            />
            {value.trackGroups.map((group) => (
              <FieldSet key={group.id}>
                <FieldLegend>{group.name}</FieldLegend>
                <NameEditor
                  key={group.name}
                  label={message("Name for track group {groupId}", { groupId: plainNumber(group.id) })}
                  initial={group.name}
                  action={message("Rename group {group}", { group: plainNumber(group.id) })}
                  onSubmit={(name) =>
                    apply(() => ({
                      type: "renameTrackGroup",
                      id: group.id,
                      name,
                    }))
                  }
                />
                <Choice
                  label={message("Parent of {groupName}", { groupName: group.name })}
                  value={String(value.groupParents[group.id] ?? "none")}
                  items={groupOptions.filter(
                    (item) => item.value !== String(group.id)
                  )}
                  onChange={(parent) =>
                    apply(() => ({
                      type: "moveTrackGroup",
                      id: group.id,
                      parent: parent === "none" ? null : Number(parent),
                    }))
                  }
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    apply(() => ({ type: "removeTrackGroup", id: group.id }))
                  }
                >
                  {t("Remove group")}{" "}{group.name}
                </Button>
              </FieldSet>
            ))}
            <FieldDescription>
              {t("Removing a group retains its tracks and promotes children to its parent.")}
            </FieldDescription>
          </FieldSet>
          <FieldSet>
            <FieldLegend>{t("Track membership and links")}</FieldLegend>
            {tracks.map((track) => (
              <FieldSet key={track.id}>
                <FieldLegend>{track.name}</FieldLegend>
                <Choice
                  label={message("Group for {trackName}", { trackName: track.name })}
                  value={String(value.trackParents[track.id] ?? "none")}
                  items={groupOptions}
                  onChange={(parent) =>
                    apply(() => ({
                      type: "moveTrack",
                      id: track.id,
                      parent: parent === "none" ? null : Number(parent),
                    }))
                  }
                />
                <Choice
                  label={message("Source for {trackName}", { trackName: track.name })}
                  value={linkValue(value.linkedTracks[track.id])}
                  items={linkChoices}
                  onChange={(value) =>
                    apply(() => {
                      if (value === "none")
                        return { type: "linkTrack", id: track.id, kind: null }
                      const [type, id] = value.split(":")
                      return {
                        type: "linkTrack",
                        id: track.id,
                        kind:
                          type === "instrument"
                            ? { type: "instrument", channel: Number(id) }
                            : type === "audioTrack"
                              ? { type: "audioTrack", mixer: Number(id) }
                              : { type: "audio", source: Number(id) },
                      }
                    })
                  }
                />
              </FieldSet>
            ))}
          </FieldSet>
          <FieldSet>
            <FieldLegend>{t("Clip groups")}</FieldLegend>
            <Field>
              <FieldLabel htmlFor={clipInput}>{t("Clip IDs to group")}</FieldLabel>
              <Input
                id={clipInput}
                value={groupClips}
                onChange={(event) => setGroupClips(event.target.value)}
              />
              <FieldDescription>
                {t("Choose at least two comma-separated IDs. Available:")}{" "}
                {clips.map((clip) => `${clip.id} (${clip.name})`).join(", ") ||
                  t("none")}
                .
              </FieldDescription>
            </Field>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() =>
                apply(() => ({
                  type: "addClipGroup",
                  group: {
                    id: freshId(),
                    clips: parseReferences(groupClips, clipIds),
                  },
                }))
              }
            >
              {t("Group clips")}
            </Button>
            {value.clipGroups.map((group) => (
              <Field key={group.id}>
                <FieldDescription>
                  {t("Group")}{" "}{group.id}:{" "}
                  {group.clips
                    .map(
                      (id) =>
                        message("{id} ({name})", { id: String(id), name: clips.find((clip) => clip.id === id)?.name ?? t("Missing clip") })
                    )
                    .join(", ")}
                </FieldDescription>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    apply(() => ({ type: "removeClipGroup", id: group.id }))
                  }
                >
                  {t("Ungroup clips")}{" "}{group.id}
                </Button>
              </Field>
            ))}
            <FieldDescription>
              {t("Clip groups save associations. Grouped canvas gestures are not connected.")}
            </FieldDescription>
          </FieldSet>
          {onMakeUnique && (
            <FieldSet>
              <FieldLegend>{t("Make unique")}</FieldLegend>
              {clips
                .filter(
                  (clip) =>
                    !makeUniqueClipIds || makeUniqueClipIds.includes(clip.id)
                )
                .map((clip) => (
                  <Button
                    key={clip.id}
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      void work(() => onMakeUnique(clip.id))
                    }}
                  >
                    {t("Make")}{" "}{clip.name}{" "}{t("(clip")}{" "}{clip.id}{t(") unique")}
                  </Button>
                ))}
              <FieldDescription>
                {t("Copies the source and redirects only the chosen clip: a pattern, an audio source entry or an automation curve. Audio copies share the immutable source file.")}
              </FieldDescription>
            </FieldSet>
          )}
        </FieldGroup>
      </fieldset>
    </section>
  )
}
