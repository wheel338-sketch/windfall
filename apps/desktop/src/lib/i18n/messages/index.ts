import { backendStaticsEn, backendStaticsEs } from "./backend-statics"
import { backendTermsEn, backendTermsEs } from "./backend-terms"
import { analysisEn, analysisEs } from "./analysis"
import { analyzersEn, analyzersEs } from "./analyzers"
import { assistantEn, assistantEs } from "./assistant"
import { audioEditorEn, audioEditorEs } from "./audio-editor"
import { audioLoggerEn, audioLoggerEs } from "./audio-logger"
import { automationEn, automationEs } from "./automation"
import { backendEn, backendEs } from "./backend"
import { backupEn, backupEs } from "./backup"
import { benchEn, benchEs } from "./bench"
import { browserEn, browserEs } from "./browser"
import { channelRackEn, channelRackEs } from "./channel-rack"
import { componentsEn, componentsEs } from "./components"
import { controllersEn, controllersEs } from "./controllers"
import { descriptorsEffectsEn, descriptorsEffectsEs } from "./descriptors-effects"
import { descriptorsBandDelayEn, descriptorsBandDelayEs } from "./descriptors-band-delay"
import { descriptorsInstrumentsEn, descriptorsInstrumentsEs } from "./descriptors-instruments"
import { descriptorsSharedEn, descriptorsSharedEs } from "./descriptors-shared"
import { effectsEn, effectsEs } from "./effects"
import { eventEditorEn, eventEditorEs } from "./event-editor"
import { exportEn, exportEs } from "./export"
import { flpImportEn, flpImportEs } from "./flp-import"
import { historyEn, historyEs } from "./history"
import { historyLabelsEn, historyLabelsEs } from "./history-labels"
import { internalControllersEn, internalControllersEs } from "./internal-controllers"
import { kitDemoEn, kitDemoEs } from "./kit-demo"
import { layoutEn, layoutEs } from "./layout"
import { libEn, libEs } from "./lib"
import { loopsEn, loopsEs } from "./loops"
import { mcpEn, mcpEs } from "./mcp"
import { mixerEn, mixerEs } from "./mixer"
import { modularEn, modularEs } from "./modular"
import { notebookEn, notebookEs } from "./notebook"
import { paletteEn, paletteEs } from "./palette"
import { paramsEn, paramsEs } from "./params"
import { performanceEn, performanceEs } from "./performance"
import { performanceMediaEn, performanceMediaEs } from "./performance-media"
import { phaseMeterEn, phaseMeterEs } from "./phase-meter"
import { pianoRollEn, pianoRollEs } from "./piano-roll"
import { playlistEn, playlistEs } from "./playlist"
import { pluginsEn, pluginsEs } from "./plugins"
import { projectFilesEn, projectFilesEs } from "./project-files"
import { projectOverviewEn, projectOverviewEs } from "./project-overview"
import { sampleInstrumentsEn, sampleInstrumentsEs } from "./sample-instruments"
import { samplePlaybackEn, samplePlaybackEs } from "./sample-playback"
import { settingsEn, settingsEs } from "./settings"
import { sharedEn, sharedEs } from "./shared"
import { slicerEn, slicerEs } from "./slicer"
import { spectrogramEn, spectrogramEs } from "./spectrogram"
import { spectrumEn, spectrumEs } from "./spectrum"
import { stageEn, stageEs } from "./stage"
import { toolsEn, toolsEs } from "./tools"
import { touchControllersEn, touchControllersEs } from "./touch-controllers"
import { transportEn, transportEs } from "./transport"
import { updatesEn, updatesEs } from "./updates"
import { videoEn, videoEs } from "./video"
import { visualizerEn, visualizerEs } from "./visualizer"

/** Every area catalog, composed for `en`. A key lives in exactly one area; shared.ts holds the ones several areas use. */
export const areaEn = {
  ...analysisEn,
  ...analyzersEn,
  ...assistantEn,
  ...audioEditorEn,
  ...audioLoggerEn,
  ...automationEn,
  ...backendEn,
  ...backendTermsEn,
  ...backendStaticsEn,
  ...backupEn,
  ...benchEn,
  ...browserEn,
  ...channelRackEn,
  ...componentsEn,
  ...controllersEn,
  ...descriptorsEffectsEn,
  ...descriptorsBandDelayEn,
  ...descriptorsInstrumentsEn,
  ...descriptorsSharedEn,
  ...effectsEn,
  ...eventEditorEn,
  ...exportEn,
  ...flpImportEn,
  ...historyEn,
  ...historyLabelsEn,
  ...internalControllersEn,
  ...kitDemoEn,
  ...layoutEn,
  ...libEn,
  ...loopsEn,
  ...mcpEn,
  ...mixerEn,
  ...modularEn,
  ...notebookEn,
  ...paletteEn,
  ...paramsEn,
  ...performanceEn,
  ...performanceMediaEn,
  ...phaseMeterEn,
  ...pianoRollEn,
  ...playlistEn,
  ...pluginsEn,
  ...projectFilesEn,
  ...projectOverviewEn,
  ...sampleInstrumentsEn,
  ...samplePlaybackEn,
  ...settingsEn,
  ...sharedEn,
  ...slicerEn,
  ...spectrogramEn,
  ...spectrumEn,
  ...stageEn,
  ...toolsEn,
  ...touchControllersEn,
  ...transportEn,
  ...updatesEn,
  ...videoEn,
  ...visualizerEn,
} as const

export const areaEs = {
  ...analysisEs,
  ...analyzersEs,
  ...assistantEs,
  ...audioEditorEs,
  ...audioLoggerEs,
  ...automationEs,
  ...backendEs,
  ...backendTermsEs,
  ...backendStaticsEs,
  ...backupEs,
  ...benchEs,
  ...browserEs,
  ...channelRackEs,
  ...componentsEs,
  ...controllersEs,
  ...descriptorsEffectsEs,
  ...descriptorsBandDelayEs,
  ...descriptorsInstrumentsEs,
  ...descriptorsSharedEs,
  ...effectsEs,
  ...eventEditorEs,
  ...exportEs,
  ...flpImportEs,
  ...historyEs,
  ...historyLabelsEs,
  ...internalControllersEs,
  ...kitDemoEs,
  ...layoutEs,
  ...libEs,
  ...loopsEs,
  ...mcpEs,
  ...mixerEs,
  ...modularEs,
  ...notebookEs,
  ...paletteEs,
  ...paramsEs,
  ...performanceEs,
  ...performanceMediaEs,
  ...phaseMeterEs,
  ...pianoRollEs,
  ...playlistEs,
  ...pluginsEs,
  ...projectFilesEs,
  ...projectOverviewEs,
  ...sampleInstrumentsEs,
  ...samplePlaybackEs,
  ...settingsEs,
  ...sharedEs,
  ...slicerEs,
  ...spectrogramEs,
  ...spectrumEs,
  ...stageEs,
  ...toolsEs,
  ...touchControllersEs,
  ...transportEs,
  ...updatesEs,
  ...videoEs,
  ...visualizerEs,
} satisfies Record<keyof typeof areaEn, unknown>
