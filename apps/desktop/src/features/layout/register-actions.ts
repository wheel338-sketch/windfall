import { registerAnalysisActions } from "@/features/analysis/actions"
import { registerBrowserActions } from "@/features/browser/actions"
import { registerChannelRackActions } from "@/features/channel-rack/actions"
import { registerMixerActions } from "@/features/mixer/actions"
import { registerPianoRollActions } from "@/features/piano-roll/actions"
import { registerPlaylistActions } from "@/features/playlist/actions"
import { registerStageActions } from "@/features/stage/actions"
import { registerSpectrumActions } from "@/features/spectrum/actions"
import { registerProjectOverviewActions } from "@/features/project-overview/actions"
import { registerPhaseMeterActions } from "@/features/phase-meter/actions"
import { registerSpectrogramActions } from "@/features/spectrogram/actions"
import { registerBuiltinActions } from "@/lib/actions/builtin"
import { registerPluginActions } from "@/features/plugins/store"
import { registerToolsActions } from "@/features/tools/actions"
import { registerTouchControllerActions } from "@/features/touch-controllers/actions"

/**
 * Puts every action the app has in the registry: the shell's own and each
 * panel's. Returns a function that removes them again.
 */
export function registerAllActions(): () => void {
  const stops = [
    registerBuiltinActions(),
    registerPluginActions(),
    registerBrowserActions(),
    registerChannelRackActions(),
    registerMixerActions(),
    registerPianoRollActions(),
    registerPlaylistActions(),
    registerAnalysisActions(),
    registerStageActions(),
    registerSpectrumActions(),
    registerProjectOverviewActions(),
    registerPhaseMeterActions(),
    registerSpectrogramActions(),
    registerToolsActions(),
    registerTouchControllerActions(),
  ]
  return () => {
    for (const stop of stops) stop()
  }
}
