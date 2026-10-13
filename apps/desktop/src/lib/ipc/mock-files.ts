import type { PluginAnnotation, PluginDatabaseView } from "@/bindings"
import { sim } from "./sim/wasm"
import { FILES_NATIVE_ONLY, type ProjectFilesBackend } from "./files"
import { translatedText as t } from "@/lib/i18n"

export function mockNativeFiles(): Omit<ProjectFilesBackend, "scoreCapture" | "scoreApply" | "scoreRead" | "scoreWrite" | "pickScorePath"> {
  const annotations: Record<string, PluginAnnotation> = {}
  const view = () => sim.call<PluginDatabaseView>("plugin_database_view", 0, annotations)
  const unavailable = async (): Promise<never> => { throw new Error(t(FILES_NATIVE_ONLY)) }
  return {
    presetCaptureChannel: unavailable, presetCapturePlugin: unavailable,
    presetApplyPlugin: unavailable, pluginsInsert: unavailable,
    presetRead: unavailable, presetWrite: unavailable, presetApplyChannel: unavailable,
    presetLibrary: async () => [], presetLibrarySave: unavailable, presetLibraryDelete: unavailable,
    templates: unavailable, templateSave: unavailable, templateNew: unavailable, templateDelete: unavailable,
    pickPresetPath: unavailable,
    pluginDatabase: async () => view(),
    pluginDatabaseAnnotate: async (format, id, annotation) => {
      if (!view().records.some((record) => record.format === format && record.id === id)) throw new Error(t("This plugin is no longer in the database"))
      const category = annotation.category?.trim().replace(/\s+/g, " ").slice(0, 48) || null
      annotations[`${format}:${id}`] = { favorite: annotation.favorite, category }
      return view()
    },
    pluginDatabaseForgetMissing: async () => view(),
    pluginsRemoveFolder: unavailable, pluginsRescan: unavailable,
  }
}
