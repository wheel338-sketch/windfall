import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useId } from "react"
import { useShallow } from "zustand/react/shallow"

import type { Channel } from "@/bindings"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { useProjectStore } from "@/lib/store/project"

import { setLayer, setLayerChild } from "../layer"
import { layerOf, MAX_LAYER_CHILDREN } from "../voice"
import { Section, SwitchRow } from "./parts"

/**
 * Turns the channel into a layer and picks the channels it plays. A layer
 * is silent itself; its notes sound on every checked channel.
 */
export function LayerSection({ channel }: { channel: Channel }) {
  useLanguageStore((state) => state.language)
  const id = useId()
  const layer = layerOf(channel.voice)
  // Two flat lists, so the selectors return equal values between edits.
  const ids = useProjectStore(
    useShallow((state) => state.project.channels.map((other) => other.id))
  )
  const names = useProjectStore(
    useShallow((state) => state.project.channels.map((other) => other.name))
  )
  const full = layer.children.length >= MAX_LAYER_CHILDREN

  return (
    <Section title={t("Layer")}>
      <SwitchRow
        label={t("Layer channel")}
        hint={t("Layer channel: plays this channel's notes on the channels chosen below. The channel makes no sound of its own")}
        checked={layer.enabled}
        onCheckedChange={(enabled) => void setLayer(channel, { ...layer, enabled })}
      />
      {layer.enabled && (
        <FieldGroup>
          {ids.length <= 1 ? (
            <p className="text-muted-foreground">{t("Add another channel for this layer to play.")}</p>
          ) : (
            ids.map((other, index) => {
              if (other === channel.id) return null
              const checked = layer.children.includes(other)
              return (
                <Field key={other} orientation="horizontal">
                  <Checkbox
                    id={`${id}-${other}`}
                    checked={checked}
                    disabled={!checked && full}
                    onCheckedChange={(next) =>
                      void setLayerChild(channel, other, next === true)
                    }
                  />
                  <FieldLabel htmlFor={`${id}-${other}`}>
                    {i18nMessage("Play on {value0}", { value0: names[index] })}
                  </FieldLabel>
                </Field>
              )
            })
          )}
          <FieldDescription>
            {i18nMessage("Notes of this channel play on the checked channels, up to {value0}. Its own sample or instrument is silent, and muting it silences the layer. A layer inside a layer passes the notes on.", { value0: MAX_LAYER_CHILDREN })}
          </FieldDescription>
        </FieldGroup>
      )}
    </Section>
  )
}
