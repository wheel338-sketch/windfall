import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type { TimeSignature } from "@/bindings"
import { useHint } from "@/lib/store/hint"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useSettings } from "@/lib/store/selectors"

import { nextDenominatorScale } from "./denominator-scale"
import { nextNumeratorScale } from "./numerator-scale"
import { nextSignaturePreset } from "./signature-preset-step"
import { nextSignature, SIGNATURE_PRESETS } from "./signature-presets"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const NUMERATORS = Array.from({ length: 16 }, (_, index) => index + 1)
const DENOMINATORS = [2, 4, 8, 16]

function NumberSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: number
  options: number[]
  onChange(value: number): void
}) {
  return (
    <Select
      value={value}
      onValueChange={(next: number | null) => {
        if (next !== null) onChange(next)
      }}
    >
      <SelectTrigger aria-label={label} className="w-16">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {options.map((option) => (
            <SelectItem key={option} value={option}>
              {option}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

/** The time signature, shown in the display and edited in a small popover. */
export function TimeSignatureField() {
  const { timeSignature } = useSettings()
  const hint = useHint(
    t("Time signature: beats per bar and the note that is one beat")
  )

  function change(next: TimeSignature) {
    void dispatch({ type: "updateSettings", patch: { timeSignature: next } })
  }

  return (
    <Popover>
      <PopoverTrigger
        aria-label={message("Time signature: {numerator} over {denominator}", { numerator: plainNumber(timeSignature.numerator), denominator: plainNumber(timeSignature.denominator) })}
        className="flex h-6 items-center rounded-sm px-1 font-readout text-[0.8125rem] font-medium text-display-foreground outline-none hover:bg-display-foreground/10 focus-visible:ring-1 focus-visible:ring-brand aria-expanded:bg-display-foreground/10"
        {...hint}
      >
        {timeSignature.numerator}/{timeSignature.denominator}
      </PopoverTrigger>
      <PopoverContent className="w-auto gap-3" align="center">
        <PopoverHeader>
          <PopoverTitle className="text-xs">{t("Time signature")}</PopoverTitle>
        </PopoverHeader>
        <div className="flex items-center gap-2">
          <NumberSelect
            label={t("Beats per bar")}
            value={timeSignature.numerator}
            options={NUMERATORS}
            onChange={(numerator) => change({ ...timeSignature, numerator })}
          />
          <span className="text-muted-foreground" aria-hidden>
            /
          </span>
          <NumberSelect
            label={t("Beat unit")}
            value={timeSignature.denominator}
            options={DENOMINATORS}
            onChange={(denominator) =>
              change({ ...timeSignature, denominator })
            }
          />
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("Halve beats per bar")}
            disabled={
              nextNumeratorScale(timeSignature.numerator, "half") === null
            }
            onClick={() => {
              const latest =
                useProjectStore.getState().project.settings.timeSignature
              const next = nextNumeratorScale(latest.numerator, "half")
              if (next !== null) {
                void dispatch({
                  type: "updateSettings",
                  patch: {
                    timeSignature: {
                      numerator: next,
                      denominator: latest.denominator,
                    },
                  },
                })
              }
            }}
          >
            {t("Halve beats")}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("Double beats per bar")}
            disabled={
              nextNumeratorScale(timeSignature.numerator, "double") === null
            }
            onClick={() => {
              const latest =
                useProjectStore.getState().project.settings.timeSignature
              const next = nextNumeratorScale(latest.numerator, "double")
              if (next !== null) {
                void dispatch({
                  type: "updateSettings",
                  patch: {
                    timeSignature: {
                      numerator: next,
                      denominator: latest.denominator,
                    },
                  },
                })
              }
            }}
          >
            {t("Double beats")}
          </Button>
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("Halve beat unit")}
            disabled={
              nextDenominatorScale(timeSignature.denominator, "half") === null
            }
            onClick={() => {
              const latest =
                useProjectStore.getState().project.settings.timeSignature
              const next = nextDenominatorScale(latest.denominator, "half")
              if (next !== null) {
                void dispatch({
                  type: "updateSettings",
                  patch: {
                    timeSignature: {
                      numerator: latest.numerator,
                      denominator: next,
                    },
                  },
                })
              }
            }}
          >
            {t("Halve unit")}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("Double beat unit")}
            disabled={
              nextDenominatorScale(timeSignature.denominator, "double") === null
            }
            onClick={() => {
              const latest =
                useProjectStore.getState().project.settings.timeSignature
              const next = nextDenominatorScale(latest.denominator, "double")
              if (next !== null) {
                void dispatch({
                  type: "updateSettings",
                  patch: {
                    timeSignature: {
                      numerator: latest.numerator,
                      denominator: next,
                    },
                  },
                })
              }
            }}
          >
            {t("Double unit")}
          </Button>
        </div>
        <div className="grid grid-cols-4 gap-1">
          {SIGNATURE_PRESETS.map((preset) => {
            const next = nextSignature(timeSignature, preset)

            return (
              <Button
                key={preset.label}
                type="button"
                variant="outline"
                size="sm"
                disabled={next === null}
                onClick={() => {
                  if (next !== null) change(next)
                }}
              >
                {preset.label}
              </Button>
            )
          })}
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("Choose the previous time signature preset")}
            disabled={nextSignaturePreset(timeSignature, "previous") === null}
            onClick={() => {
              const latest =
                useProjectStore.getState().project.settings.timeSignature
              const next = nextSignaturePreset(latest, "previous")
              if (next !== null) {
                void dispatch({
                  type: "updateSettings",
                  patch: { timeSignature: next },
                })
              }
            }}
          >
            {t("Previous")}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={t("Choose the next time signature preset")}
            disabled={nextSignaturePreset(timeSignature, "next") === null}
            onClick={() => {
              const latest =
                useProjectStore.getState().project.settings.timeSignature
              const next = nextSignaturePreset(latest, "next")
              if (next !== null) {
                void dispatch({
                  type: "updateSettings",
                  patch: { timeSignature: next },
                })
              }
            }}
          >
            {t("Next")}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
