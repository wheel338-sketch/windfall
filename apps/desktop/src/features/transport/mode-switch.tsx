import type { PlayMode } from "@/bindings"
import { Kbd } from "@/components/ui/kbd"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { runAction, useShortcutLabel } from "@/lib/actions"
import { useTransportStore } from "@/lib/store/transport"
import { cn } from "@/lib/utils"
import { translatedText as t } from "@/lib/i18n"

const MODES: {
  mode: PlayMode
  label: string
  action: string
  about: string
}[] = [
  {
    mode: "pattern",
    get label() { return t("Pattern") },
    action: "transport.patternMode",
    get about() { return t("Loop the selected pattern") },
  },
  {
    mode: "song",
    get label() { return t("Song") },
    action: "transport.songMode",
    get about() { return t("Play the playlist") },
  },
]

/** Chooses what Play plays: the selected pattern on a loop, or the song. */
export function ModeSwitch() {
  const mode = useTransportStore((state) => state.mode)
  const toggleShortcut = useShortcutLabel("transport.toggleMode")

  return (
    <div
      role="group"
      aria-label={t("What Play plays")}
      className="flex h-7 items-center rounded-md bg-muted p-0.5"
    >
      {MODES.map((item) => (
        <Tooltip key={item.mode}>
          <TooltipTrigger
            aria-pressed={mode === item.mode}
            onClick={() => void runAction(item.action)}
            className={cn(
              "h-6 rounded-[5px] px-2.5 text-xs font-medium text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50",
              mode === item.mode &&
                "bg-background text-foreground shadow-xs dark:bg-accent"
            )}
          >
            {item.label}
          </TooltipTrigger>
          <TooltipContent side="bottom">
            {item.about}
            {toggleShortcut && <Kbd>{toggleShortcut}</Kbd>}
          </TooltipContent>
        </Tooltip>
      ))}
    </div>
  )
}
