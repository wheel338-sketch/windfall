import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ParamGroup, type ParamBinder } from "@/features/params"
import { translatedText as t } from "@/lib/i18n"

export function PackBrowser({ bind }: { bind: ParamBinder }) {
  const [search, setSearch] = useState("")
  const patch = bind("patch")
  const query = search.trim().toLowerCase()
  return <ParamGroup title="Windfall patch browser">
    <FieldGroup><Field><FieldLabel htmlFor="windfall-pack-search">{t("Search patches")}</FieldLabel>
      <Input id="windfall-pack-search" value={search} onChange={(event) => setSearch(event.target.value)} />
    </Field></FieldGroup>
    {["Ember", "Prism", "Meadow"].map((bank) => <ParamGroup key={bank} title={`${bank} pack`}>
      <div className="flex flex-wrap gap-1">{patch.info.choices.map((choice, index) => choice.label.startsWith(`${bank} /`) && choice.label.toLowerCase().includes(query) ?
        <Button key={choice.value} variant="outline" size="sm" aria-pressed={index === patch.value} onClick={() => patch.onValueChange(index)}>{choice.label}</Button> : null)}</div>
    </ParamGroup>)}
    {!patch.info.choices.some((choice) => choice.label.toLowerCase().includes(query)) && <p>{t("No matching Windfall patches.")}</p>}
  </ParamGroup>
}
