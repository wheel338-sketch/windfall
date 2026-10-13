import { useState } from "react"

import { useTheme } from "@/components/theme-provider"
import { Button } from "@/components/ui/button"
import { useHintStore } from "@/lib/store/hint"

import { writeParam } from "./access"
import {
  EFFECT_KINDS,
  effectDescriptor,
  INSTRUMENT_KINDS,
  instrumentDescriptor,
  type ParamDescriptor,
} from "./descriptors"
import { GenericParamEditor } from "./generic-param-editor"
import type { SetParam } from "./use-param-binding"
import { translatedText as t } from "@/lib/i18n"

export type ParamDemoEvent = {
  descriptor: string
  id: string
  value: number
  gesture: number | undefined
}

declare global {
  interface Window {
    /** Every change the editors sent, for scripted checks. */
    __paramLog?: ParamDemoEvent[]
  }
}

const DESCRIPTORS: ParamDescriptor[] = [
  ...EFFECT_KINDS.map((kind) => effectDescriptor(kind)),
  ...INSTRUMENT_KINDS.map((kind) => instrumentDescriptor(kind)),
]

function Editor({ descriptor }: { descriptor: ParamDescriptor }) {
  const [params, setParams] = useState(descriptor.defaults)
  const setParam: SetParam = (index, value, gesture) => {
    const info = descriptor.params[index]
    window.__paramLog = [
      ...(window.__paramLog ?? []),
      { descriptor: descriptor.name, id: info.id, value, gesture },
    ]
    setParams((current) => writeParam(current, info, value))
  }
  return (
    <section
      aria-label={descriptor.name}
      className="rounded-lg border bg-background p-3"
    >
      <div className="mb-2 flex items-baseline gap-3">
        <h2 className="text-sm font-medium">{descriptor.name}</h2>
        <span className="text-xs text-muted-foreground">
          {descriptor.params.length}{" "}{t("settings")}
        </span>
      </div>
      <GenericParamEditor
        descriptor={descriptor}
        params={params}
        setParam={setParam}
      />
    </section>
  )
}

/**
 * `?view=params`: every effect and instrument the core describes, each laid
 * out by `GenericParamEditor` with nothing but its descriptor. Nothing here
 * touches a project; the settings live in the page.
 */
export default function ParamDemo() {
  const { resolvedTheme, setTheme } = useTheme()
  const hint = useHintStore((state) => state.text)
  const dark = resolvedTheme === "dark"

  return (
    <main className="min-h-screen bg-chassis pb-10 text-xs text-foreground">
      <header className="sticky top-0 z-10 flex h-12 items-center gap-3 border-b bg-background px-4">
        <h1 className="text-sm font-semibold">{t("Parameter editors")}</h1>
        <p className="min-w-0 flex-1 truncate text-muted-foreground">
          {hint ?? t("Each editor is made from a descriptor alone")}
        </p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setTheme(dark ? "light" : "dark")}
        >
          {dark ? t("Light theme") : t("Dark theme")}
        </Button>
      </header>
      <div className="mx-auto flex max-w-6xl flex-col gap-3 p-4">
        {DESCRIPTORS.map((descriptor) => (
          <Editor key={descriptor.name} descriptor={descriptor} />
        ))}
      </div>
    </main>
  )
}
