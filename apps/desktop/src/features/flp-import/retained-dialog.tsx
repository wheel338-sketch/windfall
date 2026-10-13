import { useState } from "react"
import type { RetainedPluginState } from "@/bindings"
import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const EMPTY: RetainedPluginState[] = []

/** Kept in view while the imported project is open, including after reopening. */
export function ImportedSoundsButton() {
  const count = useProjectStore((s) => s.project.retainedPlugins?.length ?? 0)
  if (count === 0) return null
  return (
    <Button
      variant="ghost"
      className="h-5 shrink-0 text-warn"
      onClick={() => useUiStore.getState().openDialog("flpRetained")}
    >
      {t("Imported sounds (")}{count})
    </Button>
  )
}

function States({ states }: { states: RetainedPluginState[] }) {
  const [shown, setShown] = useState(50)
  const instruments = states.filter((s) => s.channel !== undefined).length
  return (
    <>
      <p>
        {instruments}{" "}{t("instrument states;")}{" "}{states.length - instruments}{" "}{t("effect or other states.")}
      </p>
      <div className="flex flex-col gap-2">
        {states.slice(0, shown).map((state, index) => (
          <Collapsible key={index}>
            <CollapsibleTrigger
              render={
                <Button variant="outline" className="w-full justify-start" />
              }
            >
              {state.name ?? state.internalName}
            </CollapsibleTrigger>
            <CollapsibleContent className="flex flex-col gap-1 p-2">
              <p>
                {state.channel !== undefined
                  ? message("Channel {channel}", { channel: plainNumber(state.channel) })
                  : state.track !== undefined
                    ? message("Mixer track {track}, slot {slot}", { track: plainNumber(state.track), slot: plainNumber((state.slot ?? 0) + 1) })
                    : t("Unassigned source")}
              </p>
              <p className="wrap-anywhere">
                {state.internalName}
                {state.format ? `; ${state.format}` : ""}
                {state.vendor ? `; ${state.vendor}` : ""}
              </p>
              {state.path && <p className="wrap-anywhere">{state.path}</p>}
              <p>{state.state.length}{" "}{t("original state bytes retained.")}</p>
            </CollapsibleContent>
          </Collapsible>
        ))}
        {shown < states.length && (
          <Button variant="outline" onClick={() => setShown((n) => n + 50)}>
            {t("Show 50 more")}
          </Button>
        )}
      </div>
    </>
  )
}

export function RetainedSoundsDialog() {
  const open = useUiStore((s) => s.dialog === "flpRetained")
  const states = useProjectStore((s) => s.project.retainedPlugins ?? EMPTY)
  return (
    <Dialog
      open={open}
      onOpenChange={(open) => {
        if (!open) useUiStore.getState().closeDialog()
      }}
    >
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("Imported sounds")}</DialogTitle>
          <DialogDescription>
            {t("These original plugin states are saved in this project. Unsupported instruments remain silent and effects bypassed. A compatible host and the source FL project are needed for their original sounds; automatic restoration is not available. Target ids remain descriptive after a channel or track is removed.")}
          </DialogDescription>
        </DialogHeader>
        {open && <States states={states} />}
      </DialogContent>
    </Dialog>
  )
}
