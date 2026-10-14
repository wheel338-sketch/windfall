import { PluginCatalog } from "@/features/plugins/catalog"
import { ProjectFilesPanel } from "@/features/project-files/panel"
import { useEffect } from "react"
import { FolderBackupPanel } from "@/features/backup/panel"
import { LoopBrowser } from "@/features/loops/browser"
import { useProjectGeneration } from "@/lib/store/replaced"

import { ContextActions } from "@/components/context-actions"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { openPluginManager } from "@/features/plugins/store"
import { useShortcutScope } from "@/lib/actions"
import { useTranslation } from "@/lib/i18n"

import { BackupsTab } from "./backups"
import { BrowserHeader } from "./browser-header"
import { LibraryControls, LibraryMetadataEditor } from "./library-controls"
import { flushScrollTop } from "./persist"
import { PreviewPane } from "./preview-pane"
import { ProjectTab } from "./project-tab"
import { BROWSER_MENU } from "./row-menu"
import { ensureRootsLoaded } from "./store"
import { TreeView } from "./tree-view"

/**
 * The browser: find a sound, hear it at once, and get it into the rack.
 * Tools on top, the tree of folders in the middle, the selected sound below.
 * Its state lives in a store of its own, so hiding the panel loses nothing.
 */
export default function BrowserPanel() {
  const generation = useProjectGeneration()
  const t = useTranslation()
  useEffect(() => {
    ensureRootsLoaded()
    // The scroll position is saved a moment after scrolling stops; make sure
    // the last one is not lost when the panel or the window goes away.
    window.addEventListener("pagehide", flushScrollTop)
    return () => {
      window.removeEventListener("pagehide", flushScrollTop)
      flushScrollTop()
    }
  }, [])

  const scope = useShortcutScope("browser")

  return (
    <Tabs defaultValue="library" className="h-full min-h-0 gap-0">
      <TabsList aria-label={t("Browser view")} className="m-1 h-auto shrink-0 flex-wrap group-data-horizontal/tabs:h-auto [&_[role=tab]]:h-6">
        <TabsTrigger value="library">{t("Library")}</TabsTrigger>
        <TabsTrigger value="project">{t("Project")}</TabsTrigger>
        <TabsTrigger value="plugins">{t("Plugins")}</TabsTrigger>
        <TabsTrigger value="files">{t("Files")}</TabsTrigger>
        <TabsTrigger value="backups">{t("Backups")}</TabsTrigger>
        <TabsTrigger value="loops">{t("Loops")}</TabsTrigger>
        <TabsTrigger value="cloudBackup">{t("Cloud backup")}</TabsTrigger>
      </TabsList>
      <TabsContent value="library" className="min-h-0">
        <ContextActions items={BROWSER_MENU}>
          <div
            data-slot="browser-panel"
            className="@container/browser flex h-full min-h-0 flex-col overflow-auto *:min-w-54"
            {...scope}
          >
            <BrowserHeader />
            <LibraryControls />
            <Button
              variant="ghost"
              size="sm"
              onClick={() => openPluginManager()}
            >
              {t("Browse plugins")}
            </Button>
            {/* Keep usable controls and a virtual window when zoom or a small panel
            makes them larger than the panel. The outer panel scrolls both axes. */}
            <div
              data-slot="browser-tree-window"
              className="flex min-h-32 flex-1 shrink-0 flex-col"
            >
              <TreeView />
            </div>
            <LibraryMetadataEditor />
            <PreviewPane />
          </div>
        </ContextActions>
      </TabsContent>
      <TabsContent value="project" className="min-h-0 overflow-auto">
        <ProjectTab />
      </TabsContent>
      <TabsContent value="plugins" className="min-h-0 overflow-auto"><PluginCatalog /></TabsContent>
      <TabsContent value="files" className="min-h-0 overflow-auto"><ProjectFilesPanel /></TabsContent>
      <TabsContent value="backups" className="min-h-0 overflow-auto">
        <BackupsTab />
      </TabsContent>
      <TabsContent value="loops" className="min-h-0 overflow-auto"><LoopBrowser key={generation} /></TabsContent>
      <TabsContent value="cloudBackup" className="min-h-0 overflow-auto"><FolderBackupPanel key={generation} /></TabsContent>
    </Tabs>
  )
}
