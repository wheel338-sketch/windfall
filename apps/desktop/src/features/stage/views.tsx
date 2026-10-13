import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { closeStageView, useStageView } from "./actions"
import { LargeClock } from "./large-clock"
import { LargeMasterMeter } from "./large-master-meter"
import { translatedText as t } from "@/lib/i18n"

/** Only the open display subscribes to realtime frames. */
export function StageViews() {
  const view = useStageView((state) => state.view)

  return (
    <Dialog
      open={view !== null}
      onOpenChange={(open) => {
        if (!open) closeStageView()
      }}
    >
      <DialogContent className="max-h-[calc(100%-2rem)] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {view === "meter" ? t("Large master meter") : t("Large clock")}
          </DialogTitle>
          <DialogDescription>
            {view === "meter"
              ? t("Master peak levels. Click the meter to clear its clip light.")
              : t("Transport position in bars, beats and steps, and minutes and seconds.")}
          </DialogDescription>
        </DialogHeader>
        {view === "clock" && <LargeClock />}
        {view === "meter" && <LargeMasterMeter />}
      </DialogContent>
    </Dialog>
  )
}
