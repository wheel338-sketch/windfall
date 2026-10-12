import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { setLanguage, useLanguageStore, useTranslation, type Language } from "@/lib/i18n"

export function LanguageSection() {
  const language = useLanguageStore((state) => state.language)
  const t = useTranslation()
  const items = [{ value: "en", label: "English" }, { value: "es", label: "Español" }]
  return <section className="flex flex-col gap-2">
    <h3 className="text-xs font-medium">{t("Language")}</h3>
    <FieldGroup><Field>
      <FieldLabel>{t("Interface language")}</FieldLabel>
      <Select items={items} value={language} onValueChange={(next: Language | null) => { if (next !== null) setLanguage(next) }}>
        <SelectTrigger aria-label={t("Interface language")}><SelectValue /></SelectTrigger>
        <SelectContent><SelectGroup>{items.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
      </Select>
      <FieldDescription>{t("Language changes apply immediately to translated controls in every window.")}</FieldDescription>
    </Field></FieldGroup>
  </section>
}
