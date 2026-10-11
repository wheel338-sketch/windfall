import type { StereoEnhancerParams } from "@/bindings"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"

import type { EditorProps } from "../editor-props"
import { describeOffset } from "./stereo"
import { StereoField } from "./stereo-field"

/**
 * The stereo enhancer: width and balance, the phase between the sides (a
 * short delay of one of them, and turning one upside down), and the bass
 * it keeps in the middle.
 */
export function StereoEnhancerEditor({
  bind,
}: EditorProps<StereoEnhancerParams>) {
  const width = bind.value("width")
  const balance = bind.value("balance")
  const offset = bind.value("phaseOffsetMs")
  return (
    <div
      data-slot="stereo-enhancer-editor"
      role="group"
      aria-label="Stereo Enhancer settings"
      className="flex min-w-0 flex-col gap-2"
    >
      <StereoField
        width={width}
        balance={balance}
        label={`Stereo image at ${Math.round(width * 100)}% of its width. ${describeOffset(offset)}`}
      />
      <div className="grid grid-cols-[repeat(auto-fit,minmax(9rem,1fr))] items-start gap-2">
        <ParamGroup title="Image">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("width")}
              description="0 is mono, 1 leaves the image as it is, 2 doubles the difference between the sides"
            />
            <ParamControl
              {...bind("balance")}
              description="Turns the other side down. The side it favors keeps its level"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup
          title="Phase"
          aside={
            <span
              data-slot="phase-readout"
              className="truncate text-[10px] text-muted-foreground"
            >
              {describeOffset(offset)}
            </span>
          }
        >
          <ParamRow columns={2}>
            <ParamControl
              {...bind("phaseOffsetMs")}
              label="Offset"
              description="Holds one side back: the right above zero, the left below. The other side is not delayed"
            />
            <ParamControl
              {...bind("invert")}
              description="Turns one side upside down"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Mono bass">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("bassHz")}
              label="Below"
              description="Below this frequency the sides are brought together"
            />
            <ParamControl
              {...bind("bassLock")}
              label="Lock"
              description="How firmly the bass is kept in the middle"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Output">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("mix")}
              description="Balance between the untouched sound and the enhanced one"
            />
          </ParamRow>
        </ParamGroup>
      </div>
    </div>
  )
}
