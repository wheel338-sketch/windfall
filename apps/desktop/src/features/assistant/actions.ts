import { registry } from "@/lib/actions"
import { openAssistant, openScripts } from "./store"

export function registerAssistantActions() {
  return registry.register([
    { id: "help.assistant", title: "Offline assistant", section: "Help", keywords: "chat help tempo pattern channel proposal", run: openAssistant },
    { id: "tools.scripts", title: "Script editor", section: "Tools", keywords: "piano notes controller MIDI note transform scripting", run: openScripts },
  ])
}
