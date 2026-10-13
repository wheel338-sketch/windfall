import { useMemo } from "react"

import { cn } from "@/lib/utils"
import { message, useTranslation } from "@/lib/i18n"

import type { ParamDescriptor } from "./descriptors"
import { paramGroups } from "./groups"
import { ParamControl, type ParamControlSize } from "./param-control"
import { ParamGroup, ParamRow } from "./param-group"
import {
  useParamBinding,
  type ParamBindingOptions,
  type SetParam,
} from "./use-param-binding"

type GenericParamEditorProps<Params extends object> = {
  descriptor: ParamDescriptor<Params>
  /** The stored settings. Without them every control is disabled. */
  params: Params | null | undefined
  setParam: SetParam
  /** What each setting adds to its control: see `useParamBinding`. */
  contextItems?: ParamBindingOptions<Params>["contextItems"]
  live?: ParamBindingOptions<Params>["live"]
  marker?: ParamBindingOptions<Params>["marker"]
  size?: ParamControlSize
  className?: string
}

/**
 * An editor for any effect or instrument, made from its descriptor alone:
 * every setting gets its control, sorted into titled groups by where it
 * lives in the settings. A new processor is usable the day it has a
 * descriptor; a purpose-built layout can replace this later.
 */
export function GenericParamEditor<Params extends object>({
  descriptor,
  params,
  setParam,
  contextItems,
  live,
  marker,
  size = "md",
  className,
}: GenericParamEditorProps<Params>) {
  const t = useTranslation()
  const bind = useParamBinding({
    descriptor,
    params,
    setParam,
    contextItems,
    live,
    marker,
  })
  const groups = useMemo(() => paramGroups(descriptor), [descriptor])

  return (
    <div
      data-slot="param-editor"
      aria-label={message("{name} settings", { name: t(descriptor.name) })}
      role="group"
      className={cn(
        "grid grid-cols-[repeat(auto-fill,minmax(13.5rem,1fr))] items-start gap-2",
        className
      )}
    >
      {groups.map((group) => {
        const row = (
          <ParamRow>
            {group.params.map(({ info, label }) => (
              <ParamControl
                key={info.id}
                {...bind(info.id)}
                label={label}
                size={size}
              />
            ))}
          </ParamRow>
        )
        return group.title === null ? (
          <div key={group.key} className="col-span-full">
            {row}
          </div>
        ) : (
          <ParamGroup key={group.key} title={group.title}>
            {row}
          </ParamGroup>
        )
      })}
    </div>
  )
}
