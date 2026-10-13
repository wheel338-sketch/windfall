import { registry } from "@/lib/actions"
import { openAssistant, openScripts } from "./store"
import { dynamicTitle } from "@/lib/i18n"

export function registerAssistantActions() {
  return registry.register([
    { id: "help.assistant", ...dynamicTitle("Offline assistant", {}), section: "Help", keywords: "chat help tempo pattern channel proposal", run: openAssistant },
    { id: "tools.scripts", ...dynamicTitle("Script editor", {}), section: "Tools", keywords: "piano notes controller MIDI note transform scripting", run: openScripts },
  ])
}
