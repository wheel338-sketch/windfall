import { registry, type Action } from "@/lib/actions"

import { useTouchControllersStore } from "./store"
import { dynamicTitle } from "@/lib/i18n"

export function closeTouchControllers() {
  useTouchControllersStore.getState().close()
}

const TOUCH_CONTROLLER_ACTIONS: Action[] = [
  {
    id: "view.touchKeyboard",
    ...dynamicTitle("Touch keyboard", {}),
    section: "View",
    keywords: "on-screen piano keys touch screen controllers play audition",
    run: () => useTouchControllersStore.getState().show("keyboard"),
  },
  {
    id: "view.touchPads",
    ...dynamicTitle("Touch pads", {}),
    section: "View",
    keywords: "on-screen drum pads touch screen controllers finger drumming",
    run: () => useTouchControllersStore.getState().show("pads"),
  },
]

export function registerTouchControllerActions(): () => void {
  const unregister = registry.register(TOUCH_CONTROLLER_ACTIONS)
  return () => {
    unregister()
    closeTouchControllers()
  }
}
