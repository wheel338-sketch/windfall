import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useId, useRef, useState } from "react"

import type { ProjectSettings, SettingsPatch } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"

import { projectNameChange } from "./project-name"

const FIELDS = [
  { key: "author", label: "Author", cap: 256 },
  { key: "genre", label: "Genre", cap: 128 },
  { key: "comments", label: "Comments", cap: 16_384 },
] as const

export function ProjectInfoPanel({
  settings,
  onSave,
}: {
  settings: ProjectSettings
  onSave(patch: SettingsPatch): Promise<boolean>
}) {
  useLanguageStore((state) => state.language)
  const id = useId()
  const [draft, setDraft] = useState({
    name: settings.name,
    author: settings.author ?? "",
    genre: settings.genre ?? "",
    comments: settings.comments ?? "",
  })
  const [saving, setSaving] = useState(false)
  const pending = useRef(false)
  const [error, setError] = useState<string | null>(null)
  const invalid = (key: keyof typeof draft, cap: number) =>
    new TextEncoder().encode(draft[key].trim()).length > cap
  const nameChange = projectNameChange(settings.name, draft.name)
  const nameError = nameChange.ok
    ? null
    : nameChange.reason === "empty"
      ? t("Name must not be blank.")
      : t("Name must be at most 256 bytes.")
  const changed =
    (nameChange.ok && nameChange.name !== null) ||
    FIELDS.some(({ key }) => draft[key].trim() !== (settings[key] ?? ""))
  const tooLong = FIELDS.some(({ key, cap }) => invalid(key, cap))

  const save = async () => {
    if (pending.current || !changed || !nameChange.ok || tooLong) return
    const patch: SettingsPatch = {}
    if (nameChange.name !== null) patch.name = nameChange.name
    for (const { key } of FIELDS) {
      const value = draft[key].trim()
      if (value !== (settings[key] ?? "")) patch[key] = value
    }
    pending.current = true
    setSaving(true)
    setError(null)
    try {
      if (!(await onSave(patch))) setError(t("Could not save project info."))
    } catch {
      setError(t("Could not save project info."))
    } finally {
      pending.current = false
      setSaving(false)
    }
  }

  return (
    <form
      aria-label={t("Project info")}
      aria-busy={saving}
      onSubmit={(event) => {
        event.preventDefault()
        void save()
      }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      <FieldGroup>
        <Field data-disabled={saving} data-invalid={!!nameError}>
          <FieldLabel htmlFor={`${id}-name`}>{t("Name")}</FieldLabel>
          <Input
            id={`${id}-name`}
            value={draft.name}
            disabled={saving}
            aria-invalid={!!nameError}
            aria-describedby={nameError ? `${id}-name-error` : undefined}
            onChange={(event) =>
              setDraft((current) => ({ ...current, name: event.target.value }))
            }
          />
          {nameError && (
            <FieldError id={`${id}-name-error`}>{nameError}</FieldError>
          )}
        </Field>
        {FIELDS.map(({ key, label, cap }) => {
          const Control = key === "comments" ? Textarea : Input
          const overCap = invalid(key, cap)
          return (
            <Field key={key} data-disabled={saving} data-invalid={overCap}>
              <FieldLabel htmlFor={`${id}-${key}`}>{t(label)}</FieldLabel>
              <Control
                id={`${id}-${key}`}
                value={draft[key]}
                disabled={saving}
                aria-invalid={overCap}
                aria-describedby={overCap ? `${id}-${key}-error` : undefined}
                onChange={(event) =>
                  setDraft((current) => ({
                    ...current,
                    [key]: event.target.value,
                  }))
                }
              />
              {overCap && (
                <FieldError id={`${id}-${key}-error`}>
                  {message("{label} must be at most {cap} bytes.", { label: t(label), cap })}
                </FieldError>
              )}
            </Field>
          )
        })}
        {error && <FieldError role="alert">{error}</FieldError>}
        <Button
          type="submit"
          size="sm"
          disabled={saving || !changed || !nameChange.ok || tooLong}
        >
          {saving ? t("Saving…") : t("Save project info")}
        </Button>
      </FieldGroup>
    </form>
  )
}
