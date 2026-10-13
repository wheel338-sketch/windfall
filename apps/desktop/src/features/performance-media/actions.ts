import { registry } from "@/lib/actions"
import { useMediaWindows } from "./store"
import { dynamicTitle } from "@/lib/i18n"

export function registerPerformanceMediaActions() {
  const stop = registry.register([
    { id: "view.performance", ...dynamicTitle("Show performance grid", {}), section: "View", keywords: "launch clip pad scene live midi controller touch", run: () => useMediaWindows.setState({ performance: true }) },
    { id: "view.visualStage", ...dynamicTitle("Show visual stage", {}), section: "View", keywords: "visualizer video audio reactive moss dance animation export", run: () => useMediaWindows.setState({ visualizer: true }) },
    { id: "view.timelineVideo", ...dynamicTitle("Show timeline video", {}), section: "View", keywords: "movie local player sync presentation clock", run: () => useMediaWindows.setState({ video: true }) },
  ])
  return () => { stop(); useMediaWindows.setState({ performance: false, visualizer: false, video: false }) }
}
