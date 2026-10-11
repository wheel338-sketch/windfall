import { create } from "zustand"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { registry } from "@/lib/actions"
import { PluginCatalog } from "./catalog"
export const usePluginPicker = create(() => ({ open: false }))
export function PluginPicker() {
  const open = usePluginPicker((s) => s.open)
  return <Dialog open={open} onOpenChange={(open) => usePluginPicker.setState({ open })}><DialogContent className="flex h-dvh max-w-none! flex-col rounded-none"><DialogHeader><DialogTitle>Plugin picker</DialogTitle><DialogDescription>Search or filter favorites. Double-click a tile to add it. Drag instruments to a channel or effects to a mixer slot.</DialogDescription></DialogHeader>{open && <PluginCatalog tiles />}</DialogContent></Dialog>
}
export function registerPluginPickerActions() { return registry.register([{ id: "plugins.picker", title: "Plugin picker", section: "View", keywords: "plugin tiles grid favorites thumbnails", run: () => usePluginPicker.setState({ open: true }) }]) }
