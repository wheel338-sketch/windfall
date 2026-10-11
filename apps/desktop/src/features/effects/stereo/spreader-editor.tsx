import type { SpreaderParams } from "@/bindings"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"

import type { EditorProps } from "../editor-props"
import { StereoField } from "./stereo-field"

/**
 * The spreader: how wide, and how much of the width comes from a short
 * delay. Whatever it adds to one side it takes from the other, which is
 * why the picture can promise that mono playback is unchanged.
 */
export function SpreaderEditor({ bind }: EditorProps<SpreaderParams>) {
  const width = bind.value("width")
  return (
    <div
      data-slot="spreader-editor"
      role="group"
      aria-label="Spreader settings"
      className="flex min-w-0 flex-col gap-2"
    >
      <StereoField
        width={width}
        label={`Stereo image at ${Math.round(width * 100)}% of its width. The mono sum is unchanged`}
      />
      <div className="grid grid-cols-[repeat(auto-fit,minmax(9rem,1fr))] items-start gap-2">
        <ParamGroup title="Image">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("width")}
              description="0 is mono, 1 leaves the image as it is, 2 doubles the difference between the sides"
            />
            <ParamControl
              {...bind("mix")}
              description="Balance between the untouched sound and the spread one"
            />
          </ParamRow>
        </ParamGroup>
        <ParamGroup title="Decorrelation">
          <ParamRow columns={2}>
            <ParamControl
              {...bind("haasMs")}
              label="Delay"
              description="How far apart the two copies are that make a mono sound wide"
            />
            <ParamControl
              {...bind("amount")}
              label="Amount"
              description="How much of the delayed difference is added to one side and taken from the other"
            />
          </ParamRow>
        </ParamGroup>
      </div>
      <p
        data-slot="mono-note"
        className="text-[10px] leading-snug text-muted-foreground"
      >
        Mono playback hears the original: what is added to one side is taken
        from the other.
      </p>
    </div>
  )
}
