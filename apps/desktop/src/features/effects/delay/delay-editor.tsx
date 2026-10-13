import type { DelayParams, NoteDivision } from "@/bindings"
import { formatMs } from "@/components/audio"
import {
  formatParam,
  ParamControl,
  ParamGroup,
  ParamRow,
} from "@/features/params"
import { useTempo } from "@/lib/store"

import type { EditorProps } from "../editor-props"
import { nearestDivision, syncedDelayMs } from "./timing"
import { translatedText as t } from "@/lib/i18n"

function formatTempo(bpm: number): string {
  return `${Number.isInteger(bpm) ? bpm : bpm.toFixed(1)} BPM`
}

/**
 * The delay. Its time is a note length that follows the tempo, or a time
 * in milliseconds; whichever is set, the other is shown beside it.
 */
export function DelayEditor({ bind }: EditorProps<DelayParams>) {
  const tempo = useTempo()
  const synced = bind.value("sync") >= 0.5
  const division = bind.info("division")
  const chosen = division.choices[bind.value("division")]
  const timeMs = bind.value("timeMs")
  const label = (value: string) =>
    division.choices.find((choice) => choice.value === value)?.label ?? value

  let equivalent: string
  if (synced) {
    const ms = syncedDelayMs(chosen.value as NoteDivision, tempo)
    equivalent = `${formatMs(ms)} at ${formatTempo(tempo)}`
  } else {
    const match = nearestDivision(timeMs, tempo)
    equivalent = `${match.exact ? "" : "about "}${label(match.division)} at ${formatTempo(tempo)}`
  }

  return (
    <div
      data-slot="delay-editor"
      // Docked beside the strips the editor is about 27rem wide and 15rem
      // tall: Time and Echoes go one above the other and Stereo beside
      // them, so the whole editor is in view. Enlarged, or in a panel too
      // narrow for that, the three groups are equally wide and wrap.
      className="grid grid-cols-[repeat(auto-fit,minmax(11.5rem,1fr))] items-start gap-2 @min-[26rem]/editor:grid-cols-2 @min-[26rem]/editor:gap-1.5 in-data-enlarged:@min-[26rem]/editor:grid-cols-[repeat(auto-fit,minmax(11.5rem,1fr))] in-data-enlarged:@min-[26rem]/editor:gap-2"
    >
      <ParamGroup
        title="Time"
        aside={
          <ParamControl
            {...bind("sync")}
            layout="inline"
            label="Sync to tempo"
            description="Follow the tempo with a note length, or set a time in milliseconds"
          />
        }
      >
        <div className="flex min-h-[58px] items-center gap-3">
          {synced ? (
            <ParamControl
              {...bind("division")}
              label="Note"
              choiceStyle="select"
              className="w-28"
              description="The time between echoes as a note length"
            />
          ) : (
            <ParamControl
              {...bind("timeMs")}
              description="The time between echoes"
            />
          )}
          <p
            data-slot="delay-equivalent"
            className="min-w-0 text-[10px] leading-snug text-muted-foreground"
          >
            <span className="sr-only">
              {synced ? t("Delay time: ") : t("Closest note length: ")}
            </span>
            <span className="block font-readout text-[11px] text-foreground">
              {synced
                ? formatParam(division, bind.value("division"))
                : formatMs(timeMs)}
            </span>
            {equivalent}
          </p>
        </div>
      </ParamGroup>
      <ParamGroup title="Echoes">
        <ParamRow columns={3}>
          <ParamControl
            {...bind("feedback")}
            description="Share of each echo that makes the next one. 0% is a single echo"
          />
          <ParamControl
            {...bind("saturation")}
            label="Grit"
            description="Squashes and warms the echoes a little more with every pass"
          />
          <ParamControl
            {...bind("mix")}
            description="Balance between the dry sound and the echoes. Use 100% on a send track"
          />
          <ParamControl
            {...bind("modRateHz")}
            label="Mod rate"
            description="Speed of the modulation of echo timing"
          />
          <ParamControl
            {...bind("modDepthMs")}
            label="Mod depth"
            description="How far the echo timing moves around its selected time"
          />
        </ParamRow>
      </ParamGroup>
      <ParamGroup
        title="Stereo"
        className="@min-[26rem]/editor:col-start-2 @min-[26rem]/editor:row-span-2 @min-[26rem]/editor:row-start-1 in-data-enlarged:@min-[26rem]/editor:col-start-auto in-data-enlarged:@min-[26rem]/editor:row-span-1 in-data-enlarged:@min-[26rem]/editor:row-start-auto"
      >
        <ParamControl
          {...bind("mode")}
          label={null}
          className="w-full"
          description="Echoes keep to their side, or bounce between the sides"
        />
        <ParamRow columns={3}>
          <ParamControl
            {...bind("stereoOffsetMs")}
            label="Offset"
            description="Makes one side's echoes later than the other's, which widens the sound"
          />
          <ParamControl
            {...bind("lowCutHz")}
            description="Removes lows from each repeat, so the trail thins out"
          />
          <ParamControl
            {...bind("highCutHz")}
            description="Removes highs from each repeat, so the trail gets darker, like tape"
          />
        </ParamRow>
      </ParamGroup>
    </div>
  )
}
