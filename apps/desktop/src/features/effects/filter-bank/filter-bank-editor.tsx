import type { FilterBankParams } from "@/bindings"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import type { EditorProps } from "../editor-props"
import { PatternEditor } from "./pattern-editor"

const UNITS = Array.from({ length: 8 }, (_, index) => index)

/** Eight real audio units, each with its own drawn volume and cutoff pattern. */
export function FilterBankEditor({ bind }: EditorProps<FilterBankParams>) {
  return (
    <div role="group" aria-label="Filter Bank settings" className="flex min-w-0 flex-col gap-2">
      <ParamControl {...bind("routing")} layout="inline" />
      <p className="text-[10px] text-muted-foreground">
        Serial passes audio through units 1–8. Parallel feeds each active unit the input and averages their outputs.
        Bypass skips a unit; Dry enables an unfiltered branch for gate-only patterns.
      </p>
      <Tabs defaultValue="0">
        <TabsList aria-label="Filter units" className="w-full">
          {UNITS.map((unit) => <TabsTrigger key={unit} value={String(unit)}>Unit {unit + 1}</TabsTrigger>)}
        </TabsList>
        {UNITS.map((unit) => (
          <TabsContent key={unit} value={String(unit)} className="flex flex-col gap-2">
            <ParamGroup title={`Unit ${unit + 1} filter`}>
              <ParamControl {...bind(`slots.${unit}.mode`)} choiceStyle="select" layout="inline" />
              <ParamRow columns={2}>
                <ParamControl {...bind(`slots.${unit}.frequencyHz`)} label="Frequency" />
                <ParamControl {...bind(`slots.${unit}.q`)} label="Resonance" />
              </ParamRow>
            </ParamGroup>
            <ParamControl {...bind(`patterns.${unit}.enabled`)} label="Repeat drawn patterns" layout="inline" />
            <ParamRow columns={2}>
              <ParamControl {...bind(`patterns.${unit}.beats`)} label="Cycle (beats)" />
              <ParamControl {...bind(`patterns.${unit}.depthOctaves`)} label="Cutoff depth (octaves)" />
            </ParamRow>
            <PatternEditor bind={bind} unit={unit} kind="gate" />
            <PatternEditor bind={bind} unit={unit} kind="cutoff" />
          </TabsContent>
        ))}
      </Tabs>
      <p className="text-[10px] text-muted-foreground">
        Eight equally spaced points repeat with a smooth return to point 1. Cutoff points are bipolar around the base frequency.
        Cycles follow project tempo and start when the processor starts; each unit has its own cycle length.
      </p>
    </div>
  )
}
