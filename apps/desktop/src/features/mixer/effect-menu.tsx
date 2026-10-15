import { Add01Icon, MoreHorizontalIcon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { effectFamilies } from "@/features/plugins/families"
import { openPluginManager, usePluginBinding } from "@/features/plugins/store"
import type { ReactNode } from "react"

import type { EffectId, EffectKind, TrackId } from "@/bindings"
import { ActionMenuItem, MenuTick } from "@/components/action-menu-item"
import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { EFFECT_KINDS } from "@/features/params"
import {
  runAction,
  shortcutLabel,
  useActions,
  useAppState,
} from "@/lib/actions"
import { reportError } from "@/lib/errors"
import { useHint } from "@/lib/store"
import { cn } from "@/lib/utils"

import { replaceEffectActionId } from "./effect-actions"
import { addEffect, effectName, selectEffect } from "./effect-ops"
import { translatedText as t, dynamicTitle, message } from "@/lib/i18n"

/**
 * What can be done to an effect, for the right-click menu of its slot and
 * the "…" menu of its panel. Every entry runs a registry action on the
 * selected effect, so open the menu only after selecting the effect.
 */
export function effectMenu(kind: EffectKind, hosted = false): ContextItem[] {
  return [
    "mixer.openEffect",
    "mixer.bypassEffect",
    contextSeparator,
    "mixer.duplicateEffect",
    "mixer.moveEffectUp",
    "mixer.moveEffectDown",
    {
      submenu: t("Replace with"),
      items: EFFECT_KINDS.map((other) => ({
        title: effectName(other),
        disabled: !hosted && other === kind,
        run: () => runAction(replaceEffectActionId(other)),
      })),
    },
    ...(!hosted ? ["mixer.resetEffect"] : []),
    contextSeparator,
    {
      ...dynamicTitle("Remove effect", {}),
      destructive: true,
      // The key works while a slot or an effect's header has the focus.
      shortcut: shortcutLabel("mixer.removeEffect"),
      run: () => runAction("mixer.removeEffect"),
    },
  ]
}

// Rendered only while a menu is open, so following the whole app state
// here costs nothing the rest of the time.
function MenuItems({
  items,
  inset,
}: {
  items: ContextItem[]
  /** Leaves room for a tick, to line up with the on/off entries. */
  inset: boolean
}) {
  const actions = useActions()
  const state = useAppState()
  return items.map((item, index) => {
    if (typeof item === "string") {
      const action = actions.find((candidate) => candidate.id === item)
      return action ? (
        <ActionMenuItem
          key={item}
          action={action}
          state={state}
          inset={inset}
        />
      ) : null
    }
    if ("separator" in item) return <DropdownMenuSeparator key={index} />
    if ("label" in item || "dynamic" in item) return null
    if ("submenu" in item) {
      return (
        <DropdownMenuSub key={index}>
          <DropdownMenuSubTrigger>
            {inset && <MenuTick checked={false} />}
            {item.submenu}
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <MenuItems items={item.items} inset={false} />
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      )
    }
    return (
      <DropdownMenuItem
        key={index}
        disabled={item.disabled}
        variant={item.destructive ? "destructive" : "default"}
        onClick={() => {
          new Promise((resolve) => resolve(item.run())).catch(
            (error: unknown) => reportError(error, item.title)
          )
        }}
      >
        {inset && <MenuTick checked={false} />}
        <span className="whitespace-nowrap">{item.title}</span>
        {item.shortcut && (
          <DropdownMenuShortcut className="pl-6">
            {item.shortcut}
          </DropdownMenuShortcut>
        )}
      </DropdownMenuItem>
    )
  })
}

type EffectMenuButtonProps = {
  effect: EffectId
  kind: EffectKind
  className?: string
}

/** The "…" button of an effect's panel, with the slot's menu behind it. */
export function EffectMenuButton({
  effect,
  kind,
  className,
}: EffectMenuButtonProps) {
  const plugin = usePluginBinding({ type: "effect", effect })
  const name = plugin?.name ?? effectName(kind)
  const hint = useHint(message("Everything that can be done to this {name}", { name }))
  return (
    <DropdownMenu
      onOpenChange={(open) => {
        if (open) selectEffect(effect)
      }}
    >
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            aria-label={message("{name} actions", { name })}
            data-slot="effect-menu"
            className={cn(
              "flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground",
              className
            )}
            {...hint}
          />
        }
      >
        <HugeiconsIcon
          icon={MoreHorizontalIcon}
          strokeWidth={2}
          className="size-3.5"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-auto min-w-48">
        <MenuItems items={effectMenu(kind, !!plugin)} inset />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

type AddEffectMenuProps = {
  track: TrackId
  /** Where in the chain the new effect goes. Leave out for the end. */
  index?: number
  /** The button's content. Defaults to a plus and the word "Effect". */
  children?: ReactNode
  className?: string
}

/** Opens the list of effects that can be put on a track. */
export function AddEffectMenu({
  track,
  index,
  children,
  className,
}: AddEffectMenuProps) {
  const hint = useHint(
    t("Add an effect to this track. The signal runs through them from the top down, before the fader")
  )
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            data-slot="track-add-effect"
            aria-label={t("Add effect")}
            className={cn(
              "flex h-[18px] min-w-0 items-center gap-0.5 rounded-[3px] pr-1 pl-0.5 text-[10px] leading-none text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground",
              className
            )}
            {...hint}
          />
        }
      >
        {children ?? (
          <>
            <HugeiconsIcon
              icon={Add01Icon}
              strokeWidth={2}
              className="size-3 shrink-0"
            />
            <span className="truncate">{t("Effect")}</span>
          </>
        )}
      </DropdownMenuTrigger>
      {/* Every effect at once, in columns by family, so the list can be
          surveyed by role instead of scrolled through by name. */}
      <DropdownMenuContent className="w-[min(46rem,calc(100vw-1rem))]">
        <div className="flex flex-wrap items-center gap-x-2 px-1">
          <DropdownMenuItem onClick={() => openPluginManager(track)}>
            {t("Browse plugins…")}
          </DropdownMenuItem>
          <p className="text-[11px] text-muted-foreground">{t("Type a name to jump to it.")}</p>
        </div>
        <DropdownMenuSeparator />
        <div data-slot="effect-families" className="columns-[10.5rem] gap-2">
          {effectFamilies().map((family) => (
            <DropdownMenuGroup
              key={family.id}
              data-family={family.id}
              className="mb-1 break-inside-avoid"
            >
              <DropdownMenuLabel className="pb-0.5">
                <span className="block font-medium text-foreground">{family.label}</span>
                {family.role && <span className="block text-[10px] leading-snug">{family.role}</span>}
              </DropdownMenuLabel>
              {family.id === "utility" && (
                <DropdownMenuItem onClick={() => void addEffect(track, "sendTap", index)}>{t("Audio editor recorder")}</DropdownMenuItem>
              )}
              {family.kinds.map((kind) => (
                <DropdownMenuItem
                  key={kind}
                  onClick={() => void addEffect(track, kind, index)}
                >
                  {effectName(kind)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuGroup>
          ))}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
