/*
 * The controls every built-in instrument and effect editor is made of. An
 * editor hands in the settings and a way to change one of them; nothing here
 * knows about the project store.
 */
export {
  EFFECT_KINDS,
  effectDescriptor,
  INSTRUMENT_KINDS,
  instrumentDescriptor,
  paramIndex,
  paramInfo,
  shortFloat,
  type EffectParamsOf,
  type InstrumentParamsOf,
  type ParamDescriptor,
} from "./descriptors"
export { clampParam, readParam, writeParam } from "./access"
export { formatParam, formatParamDisplay, paramIsBipolar, paramScale, paramUnitKind, parseParam, parseParamDisplay } from "./format"
export {
  ParamControl,
  paramHint,
  type ParamControlProps,
  type ParamControlSize,
} from "./param-control"
export { SEGMENTED_MAX_CHOICES, type ChoiceIcon } from "./param-choice"
export { ParamGroup, ParamRow } from "./param-group"
export { ParamEnvelope } from "./param-envelope"
export {
  useParamBinding,
  type ParamBinder,
  type ParamBinding,
  type ParamBindingOptions,
  type ParamGroupBinding,
  type SetParam,
} from "./use-param-binding"
export { GenericParamEditor } from "./generic-param-editor"
export {
  humanizePath,
  paramGroups,
  shortLabels,
  type GroupedParam,
  type ParamGroupPlan,
} from "./groups"
