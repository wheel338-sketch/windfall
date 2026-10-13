import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

import { nextScaleRoot } from "./scale-root-step"
import { nextScale } from "./scale-step"
import { isScaleId, ROOT_NAMES, scaleDefinition, SCALES } from "./scales"
import { usePianoRollStore } from "./store"
import { message, translatedText as t } from "@/lib/i18n"

/** Independent scale shading and pitch snapping; changing either never edits notes. */
export function ScaleControls() {
  const root = usePianoRollStore((state) => state.scaleRoot)
  const id = usePianoRollStore((state) => state.scaleId)
  const highlight = usePianoRollStore((state) => state.highlightScale)
  const snap = usePianoRollStore((state) => state.snapToScale)
  const label = message("{root} {scale}", { root: ROOT_NAMES[root], scale: scaleDefinition(id).label })

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant={highlight || snap ? "secondary" : "outline"}
            size="sm"
            aria-label={message("Scale settings: {label}", { label })}
          />
        }
      >
        {t("Scale:")}{" "}{ROOT_NAMES[root].split(" / ")[0]} {scaleDefinition(id).label}
        {snap ? t(" · snap") : ""}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-80">
        <DropdownMenuGroup>
          <DropdownMenuLabel>{label}</DropdownMenuLabel>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{t("Root key")}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-40">
              <DropdownMenuRadioGroup
                value={String(root)}
                onValueChange={(value) =>
                  usePianoRollStore.getState().setScaleRoot(Number(value))
                }
              >
                <DropdownMenuLabel>{t("Root key")}</DropdownMenuLabel>
                {ROOT_NAMES.map((name, index) => (
                  <DropdownMenuRadioItem key={name} value={String(index)}>
                    {name}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem
            onClick={() => {
              const state = usePianoRollStore.getState()
              const next = nextScaleRoot(state.scaleRoot, "lower")
              if (next !== null) state.setScaleRoot(next)
            }}
          >
            {t("Lower root")}
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={() => {
              const state = usePianoRollStore.getState()
              const next = nextScaleRoot(state.scaleRoot, "higher")
              if (next !== null) state.setScaleRoot(next)
            }}
          >
            {t("Higher root")}
          </DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{t("Musical scale")}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-64">
              <DropdownMenuRadioGroup
                value={id}
                onValueChange={(value) => {
                  if (isScaleId(value))
                    usePianoRollStore.getState().setScaleId(value)
                }}
              >
                <DropdownMenuLabel>{t("Musical scale")}</DropdownMenuLabel>
                {SCALES.map((scale) => (
                  <DropdownMenuRadioItem key={scale.id} value={scale.id}>
                    {scale.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem
            onClick={() => {
              const state = usePianoRollStore.getState()
              const next = nextScale(state.scaleId, "previous")
              if (next !== null) state.setScaleId(next)
            }}
          >
            {t("Previous scale")}
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={() => {
              const state = usePianoRollStore.getState()
              const next = nextScale(state.scaleId, "next")
              if (next !== null) state.setScaleId(next)
            }}
          >
            {t("Next scale")}
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuCheckboxItem
            checked={highlight}
            onCheckedChange={(enabled) =>
              usePianoRollStore.getState().setHighlightScale(enabled)
            }
          >
            {t("Highlight scale")}
          </DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem
            checked={snap}
            onCheckedChange={(enabled) =>
              usePianoRollStore.getState().setSnapToScale(enabled)
            }
          >
            {t("Snap pitches to scale")}
          </DropdownMenuCheckboxItem>
          <DropdownMenuLabel>
            {t("Alt bypasses snap when drawing or dragging.")}
          </DropdownMenuLabel>
          <DropdownMenuLabel>
            {t("Groups snap their anchor and keep their intervals.")}
          </DropdownMenuLabel>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
