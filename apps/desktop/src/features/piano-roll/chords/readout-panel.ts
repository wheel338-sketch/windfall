import { create } from "zustand"

import { onProjectReplaced } from "@/lib/store/replaced"

/** Whether the chord display is open. View state only, never saved. */
export const useChordDisplayStore = create<{
  open: boolean
  setOpen(open: boolean): void
}>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}))

onProjectReplaced(() => useChordDisplayStore.setState({ open: false }))

export function openChordDisplay() {
  useChordDisplayStore.getState().setOpen(true)
}
