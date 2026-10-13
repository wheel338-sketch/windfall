import { useEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { dispatch, useProjectStore } from "@/lib/store/project"
import {
  getProjectGeneration,
  useProjectGeneration,
} from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { formatTempo } from "@/lib/time"
import { TapTempoEstimator, type TapMeasurement } from "./tap-tempo"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export function TapTempoDialog() {
  const open = useUiStore((state) => state.dialog === "tempoTap")
  const generation = useProjectGeneration()
  const [busy, setBusy] = useState(false)
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value && !busy) useUiStore.getState().closeDialog()
      }}
    >
      <DialogContent showCloseButton={!busy}>
        <DialogHeader>
          <DialogTitle>{t("Tap tempo")}</DialogTitle>
          <DialogDescription>
            {t("Tap in time, review the estimate, then Apply. Use Space or Enter while the Tap button is focused. Tapping changes no project data.")}
          </DialogDescription>
        </DialogHeader>
        {open && <Tapper key={generation} onBusy={setBusy} />}
      </DialogContent>
    </Dialog>
  )
}

function Tapper({ onBusy }: { onBusy(value: boolean): void }) {
  const [estimator] = useState(() => new TapTempoEstimator())
  const [measurement, setMeasurement] = useState<TapMeasurement>({
    bpm: null,
    taps: 0,
  })
  const [busy, setBusy] = useState(false)
  const alive = useRef(false)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      onBusy(false)
    }
  }, [onBusy])
  function tap() {
    setMeasurement(estimator.tap(performance.now()))
  }
  async function apply() {
    if (busy || measurement.bpm === null) return
    const generation = getProjectGeneration()
    if (
      measurement.bpm === useProjectStore.getState().project.settings.tempoBpm
    ) {
      useUiStore.getState().closeDialog()
      return
    }
    setBusy(true)
    onBusy(true)
    try {
      const result = await dispatch({
        type: "updateSettings",
        patch: { tempoBpm: measurement.bpm },
      })
      if (result && alive.current && generation === getProjectGeneration())
        useUiStore.getState().closeDialog()
    } finally {
      if (alive.current) {
        setBusy(false)
        onBusy(false)
      }
    }
  }
  return (
    <div className="flex flex-col gap-4">
      <Button
        autoFocus
        size="lg"
        disabled={busy}
        onClick={tap}
        onKeyDown={(event) => {
          if (
            (event.key === " " || event.key === "Enter") &&
            !event.altKey &&
            !event.ctrlKey &&
            !event.metaKey
          ) {
            event.preventDefault()
            if (!event.repeat) tap()
          }
        }}
        onKeyUp={(event) => {
          if (event.key === " " || event.key === "Enter") event.preventDefault()
        }}
      >
        {t("Tap")}
      </Button>
      <p role="status" className="text-center font-readout text-xl">
        {measurement.bpm === null
          ? message("{taps} {taps2} · tap at least twice", { taps: plainNumber(measurement.taps), taps2: measurement.taps === 1 ? t("tap") : t("taps") })
          : message("{taps} taps · {tempo} BPM", { taps: plainNumber(measurement.taps), tempo: formatTempo(measurement.bpm) })}
      </p>
      <p className="text-sm text-muted-foreground">
        {t("The last eight taps are averaged. Very close taps are ignored; established rhythms tolerate uneven beats. A pause longer than six seconds starts over. Reset before tapping a different rhythm.")}
      </p>
      <p className="text-sm text-muted-foreground">
        {t("Apply changes the stored project tempo in one undo step. Existing tempo automation still controls playback where its clips are active.")}
      </p>
      <div className="flex justify-between gap-2">
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => setMeasurement(estimator.reset())}
        >
          {t("Reset taps")}
        </Button>
        <div className="flex gap-2">
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => useUiStore.getState().closeDialog()}
          >
            {t("Cancel")}
          </Button>
          <Button
            disabled={busy || measurement.bpm === null}
            onClick={() => void apply()}
          >
            {busy ? t("Applying…") : t("Apply tempo")}
          </Button>
        </div>
      </div>
    </div>
  )
}
