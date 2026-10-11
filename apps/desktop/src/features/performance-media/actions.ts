import { registry } from "@/lib/actions"
import { useMediaWindows } from "./store"

export function registerPerformanceMediaActions() {
  const stop = registry.register([
    { id: "view.performance", title: "Show performance grid", section: "View", keywords: "launch clip pad scene live midi controller touch", run: () => useMediaWindows.setState({ performance: true }) },
    { id: "view.visualStage", title: "Show visual stage", section: "View", keywords: "visualizer video audio reactive moss dance animation export", run: () => useMediaWindows.setState({ visualizer: true }) },
    { id: "view.timelineVideo", title: "Show timeline video", section: "View", keywords: "movie local player sync presentation clock", run: () => useMediaWindows.setState({ video: true }) },
  ])
  return () => { stop(); useMediaWindows.setState({ performance: false, visualizer: false, video: false }) }
}
