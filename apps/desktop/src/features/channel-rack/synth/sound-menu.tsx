import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"

import { ActionMenuItem, MenuTick } from "@/components/action-menu-item"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  isChecked,
  isEnabled,
  runAction,
  useActions,
  useAppState,
} from "@/lib/actions"
import { instrumentParams } from "@/lib/channel-source"
import { useHint } from "@/lib/store/hint"

import { synthSoundActionId } from "../actions"
import { loadInstrumentSound, selectedChannel } from "../channel-ops"
import { matchingPreset, SYNTH_PRESETS, type SynthSettings } from "./presets"
import { nextSynthSoundPreset } from "./synth-sound-preset-step"
import { historyLabel, translatedText as t, message } from "@/lib/i18n"

// Rendered only while the menu is open, so it can follow the app state.
function SoundItems() {
  const actions = useActions()
  const state = useAppState()
  const find = (id: string) => actions.find((action) => action.id === id)
  const init = find("channel.initInstrument")
  const params = instrumentParams(selectedChannel())
  const previous =
    params?.type === "subtractiveSynth"
      ? nextSynthSoundPreset(params, "previous")
      : null
  const next =
    params?.type === "subtractiveSynth"
      ? nextSynthSoundPreset(params, "next")
      : null

  return (
    <>
      {init && <ActionMenuItem action={init} state={state} inset />}
      <DropdownMenuSeparator />
      {SYNTH_PRESETS.map((preset) => {
        // Each sound is a registry action. Here, under the synth's own
        // name, an item needs only the name of the sound.
        const action = find(synthSoundActionId(preset.id))
        if (!action) return null
        const checked = isChecked(action, state)
        return (
          <DropdownMenuItem
            key={preset.id}
            role="menuitemcheckbox"
            aria-checked={checked}
            disabled={!isEnabled(action, state)}
            title={preset.description}
            onClick={() => void runAction(action.id)}
          >
            <MenuTick checked={checked} />
            {preset.name}
          </DropdownMenuItem>
        )
      })}
      <DropdownMenuItem
        disabled={!previous}
        onClick={() => {
          const channel = selectedChannel()
          const params = instrumentParams(channel)
          if (!channel || params?.type !== "subtractiveSynth") return
          const next = nextSynthSoundPreset(params, "previous")
          if (!next) return
          void loadInstrumentSound(
            channel.id,
            next.params,
            historyLabel("Load sound: {name}", { name: next.name })
          )
        }}
      >
        {t("Previous sound")}
      </DropdownMenuItem>
      <DropdownMenuItem
        disabled={!next}
        onClick={() => {
          const channel = selectedChannel()
          const params = instrumentParams(channel)
          if (!channel || params?.type !== "subtractiveSynth") return
          const next = nextSynthSoundPreset(params, "next")
          if (!next) return
          void loadInstrumentSound(
            channel.id,
            next.params,
            historyLabel("Load sound: {name}", { name: next.name })
          )
        }}
      >
        {t("Next sound")}
      </DropdownMenuItem>
      <p className="px-2 py-1.5 text-muted-foreground">
        {t("A sound replaces every setting of the synth. Undo brings the old ones back.")}
      </p>
    </>
  )
}

/**
 * Names the built-in sound the synth is set to, and opens the list of them.
 * Once a setting has been moved the sound is the user's own.
 */
export function SoundMenu({ params }: { params: SynthSettings }) {
  const preset = matchingPreset(params)
  const hint = useHint(
    preset
      ? message("Sound: {presetName}. {description}. Click to load another starting point", { presetName: preset.name, description: preset.description })
      : t("Sound: your own. Click to load a starting point over it")
  )

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="xs"
            aria-label={message("Sound: {presetName}", { presetName: preset?.name ?? t("edited") })}
            className="max-w-36 shrink-0"
            {...hint}
          />
        }
      >
        <span className="truncate">{preset?.name ?? t("Edited")}</span>
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-60">
        <SoundItems />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
