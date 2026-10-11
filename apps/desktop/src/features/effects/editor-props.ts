import type { EffectId, TrackId } from "@/bindings"
import type { ParamBinder } from "@/features/params"

/**
 * What the editor of one kind of effect is given. `bind(id)` connects a
 * control to a setting; `bind.value(id)` is what that setting shows right
 * now, which is ahead of `params` while a control is being dragged.
 */
export type EditorProps<Params> = {
  /** The effect being edited, which is how its meters find their readings. */
  effect: EffectId
  /**
   * The mixer track the effect is on, which is where its live displays
   * take their audio from. Without it they stay empty.
   */
  track?: TrackId
  /** The stored settings. */
  params: Params
  bind: ParamBinder
}
