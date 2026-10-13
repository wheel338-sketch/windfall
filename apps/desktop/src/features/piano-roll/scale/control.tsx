import { useEffect, useState } from "react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

import { useSession } from "../context"
import { attachScaleHighlight } from "./highlight"
import {
  HIGHLIGHT_SCALES,
  isHighlightScaleId,
  PITCH_CLASSES,
  type ScaleHighlight,
} from "./model"
import { translatedText as t, message } from "@/lib/i18n"

/** One toolbar control; its choice disappears when the open roll unmounts. */
export function ScaleHighlightControl() {
  const session = useSession()
  const [choice, setChoice] = useState<ScaleHighlight>({
    root: 0,
    scale: "off",
  })
  useEffect(() => attachScaleHighlight(session, choice), [session, choice])
  const scale = HIGHLIGHT_SCALES.find((scale) => scale.id === choice.scale)!
  const label =
    choice.scale === "off"
      ? t("Off")
      : `${PITCH_CLASSES[choice.root]} ${scale.label}`

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant={choice.scale === "off" ? "outline" : "secondary"}
            size="sm"
            aria-label={message("Scale highlight: {label}", { label })}
          />
        }
      >
        {t("Highlight:")}{" "}{label}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64">
        <DropdownMenuGroup>
          <DropdownMenuLabel>{t("Scale highlight")}</DropdownMenuLabel>
          <DropdownMenuRadioGroup
            value={choice.scale}
            onValueChange={(scale) => {
              if (isHighlightScaleId(scale)) {
                setChoice((previous) => ({ ...previous, scale }))
              }
            }}
          >
            {HIGHLIGHT_SCALES.map((scale) => (
              <DropdownMenuRadioItem key={scale.id} value={scale.id}>
                {scale.label}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              {t("Highlight root:")}{" "}{PITCH_CLASSES[choice.root]}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              <DropdownMenuGroup>
                <DropdownMenuLabel>{t("Highlight root")}</DropdownMenuLabel>
                <DropdownMenuRadioGroup
                  value={String(choice.root)}
                  onValueChange={(value) => {
                    const root = Number(value)
                    if (Number.isInteger(root) && root >= 0 && root < 12) {
                      setChoice((previous) => ({ ...previous, root }))
                    }
                  }}
                >
                  {PITCH_CLASSES.map((name, root) => (
                    <DropdownMenuRadioItem key={name} value={String(root)}>
                      {name}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuLabel>{t("Dims pitches outside the scale.")}</DropdownMenuLabel>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
