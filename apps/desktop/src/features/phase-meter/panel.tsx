import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { closePhaseMeter, usePhaseMeterPanel } from "./actions"
import { PhaseMeterView } from "./view"
import { translatedText as t } from "@/lib/i18n"

/** Closing the panel unmounts its realtime subscriber immediately. */
export function PhaseMeterPanel() {
  const open = usePhaseMeterPanel((state) => state.open)

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closePhaseMeter()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Phase meter")}</DialogTitle>
          <DialogDescription>{t("Current stereo correlation.")}</DialogDescription>
        </DialogHeader>
        {open && <PhaseMeterView />}
      </DialogContent>
    </Dialog>
  )
}
