import { ActionButton } from "@/components/action-button"
import { useTranslation } from "@/lib/i18n"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { ContextHelp } from "./context-help"
import { GuidedTutorial } from "./guided-tutorial"
import { TUTORIALS, type TutorialKind } from "./tutorials"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import {
  closeGettingStarted,
  useGettingStartedStore,
} from "./getting-started-store"

const STEPS = [
  {
    title: "Add a channel",
    sentence: "Add an instrument channel.",
    action: "channel.add",
  },
  {
    title: "Open the piano roll",
    sentence: "Draw notes on that channel.",
    action: "view.pianoRoll",
  },
  {
    title: "Open the playlist",
    sentence: "Place the pattern on the timeline.",
    action: "view.playlist",
  },
  {
    title: "Play",
    sentence: "Start playback from the transport.",
    action: "transport.play",
  },
  {
    title: "Export",
    sentence: "Write the mix to an audio file.",
    action: "file.export",
  },
]

export function GettingStartedDialog() {
  const t = useTranslation()
  const tutorial = useGettingStartedStore((state) => state.tutorial)
  const setTutorial = (tutorial: TutorialKind | null) => useGettingStartedStore.setState({ tutorial })
  const open = useGettingStartedStore((state) => state.open)
  const context = useGettingStartedStore((state) => state.context)
  const items = (Object.keys(TUTORIALS) as TutorialKind[]).map((kind) => ({ value: kind, label: t(TUTORIALS[kind].title) }))

  return (
    <Dialog
      modal={false}
      open={open}
      onOpenChange={(next) => {
        if (!next) { closeGettingStarted(); setTutorial(null) }
      }}
    >
      <DialogContent className="max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("Getting started")}</DialogTitle>
          <DialogDescription>
            {t("Use these actions to make your first track.")}
          </DialogDescription>
        </DialogHeader>
        <ContextHelp panel={context} />
        <FieldGroup><Field><FieldLabel>{t("Choose a tutorial")}</FieldLabel>
          <Select items={items} value={tutorial} onValueChange={(value: TutorialKind | null) => setTutorial(value)}>
            <SelectTrigger aria-label={t("Choose a tutorial")}><SelectValue placeholder={t("Choose a tutorial")} /></SelectTrigger>
            <SelectContent><SelectGroup>{items.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
          </Select>
        </Field></FieldGroup>
        {tutorial ? <GuidedTutorial key={tutorial} kind={tutorial} /> : <ol className="flex list-decimal flex-col gap-4 pl-5">
          {STEPS.map((step) => (
            <li key={step.action}>
              <div className="flex flex-col items-start gap-1">
                <p className="text-muted-foreground">{t(step.sentence)}</p>
                <ActionButton action={step.action} variant="outline" size="sm">
                  {step.title}
                </ActionButton>
              </div>
            </li>
          ))}
        </ol>}
      </DialogContent>
    </Dialog>
  )
}
