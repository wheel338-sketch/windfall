import { message, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { ArrowDown01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { Fragment, type ReactNode } from "react"

import { contextSeparator, type ContextItem } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ValueContextItems } from "@/components/value-context-menu"

/*
 * Presets and arithmetic steps of a setting: set it to a preset, go to the
 * previous or next preset, halve or double it. They used to be rows of
 * buttons under every knob, which pushed the knobs themselves apart. Here
 * each setting has one small menu, and the same entries are on the
 * right-click menu of its knob.
 */

/** One thing that can be done to a setting from its menu. */
export type ValueStep = {
  title: string
  run(): void
  /** Nothing to do: already there, or at the end of the range. */
  disabled?: boolean
  /**
   * The menu stays open after this entry, so it can be picked again. For
   * stepping, as against picking a preset.
   */
  repeat?: boolean
}

/** A setting's presets and steps, in groups that are shown apart. */
export type SettingSteps = {
  /** What the steps change, as short as its knob's label. */
  name: string
  groups: readonly (readonly ValueStep[])[]
}

type Direction = "previous" | "next"
type Factor = "half" | "double"

/** "Previous preset" and "Next preset", or the titles given. */
export function directionSteps(
  run: (direction: Direction) => void,
  available: (direction: Direction) => boolean,
  titles: Record<Direction, string> = {
    previous: t("Previous preset"),
    next: t("Next preset"),
  }
): ValueStep[] {
  return (["previous", "next"] as const).map((direction) => ({
    title: titles[direction],
    run: () => run(direction),
    disabled: !available(direction),
    repeat: true,
  }))
}

/** "Half" and "Double", or the titles given. */
export function factorSteps(
  run: (factor: Factor) => void,
  available: (factor: Factor) => boolean,
  titles: Record<Factor, string> = { half: t("Half"), double: t("Double") }
): ValueStep[] {
  return (["half", "double"] as const).map((factor) => ({
    title: titles[factor],
    run: () => run(factor),
    disabled: !available(factor),
    repeat: true,
  }))
}

/** The steps as entries of a right-click menu, groups apart. */
export function stepItems(steps: SettingSteps): ContextItem[] {
  return steps.groups
    .filter((group) => group.length > 0)
    .flatMap((group, index) => [
      ...(index > 0 ? [contextSeparator] : []),
      ...group.map(({ title, run, disabled }) => ({ title, run, disabled })),
    ])
}

/**
 * Puts a setting's steps on the right-click menu of the value controls
 * inside, above the entries every control has.
 */
export function WithSteps({
  steps,
  children,
}: {
  steps: SettingSteps
  children: ReactNode
}) {
  return (
    <ValueContextItems items={stepItems(steps)}>{children}</ValueContextItems>
  )
}

/** One setting's menu: a small button named after the setting. */
function StepsMenu({ steps, disabled }: { steps: SettingSteps; disabled?: boolean }) {
  const groups = steps.groups.filter((group) => group.length > 0)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            size="xs"
            aria-label={message("{name} presets and steps", { name: steps.name })}
            disabled={disabled}
          />
        }
      >
        {steps.name}
        <HugeiconsIcon
          icon={ArrowDown01Icon}
          strokeWidth={2}
          className="text-muted-foreground"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-auto min-w-40">
        <DropdownMenuGroup>
          <DropdownMenuLabel>{steps.name}</DropdownMenuLabel>
        </DropdownMenuGroup>
        {groups.map((group, index) => (
          <Fragment key={index}>
            {index > 0 && <DropdownMenuSeparator />}
            <DropdownMenuGroup>
              {group.map((step) => (
                <DropdownMenuItem
                  key={step.title}
                  disabled={step.disabled}
                  closeOnClick={!step.repeat}
                  onClick={step.run}
                >
                  {step.title}
                </DropdownMenuItem>
              ))}
            </DropdownMenuGroup>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * One row of small menus, one for each setting, in place of the rows of
 * preset and step buttons each of them had.
 */
export function StepsMenus({
  settings,
  disabled,
}: {
  settings: readonly SettingSteps[]
  /** Settings whose menus are off, by name, such as one that has no effect now. */
  disabled?: readonly string[]
}) {
  useLanguageStore((state) => state.language)
  return (
    <div
      role="group"
      aria-label={t("Presets and steps")}
      className="flex flex-wrap gap-1"
    >
      {settings.map((steps) => (
        <StepsMenu
          key={steps.name}
          steps={steps}
          disabled={disabled?.includes(steps.name)}
        />
      ))}
    </div>
  )
}
