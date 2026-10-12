// Node 22+, Windows WebView2, prebuilt debug app. No packages or framework.
import { spawn } from "node:child_process"
import { mkdir, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const executable = resolve(process.argv[2] ?? "target/debug/windfall-desktop.exe")
const output = resolve(process.argv[3] ?? "target/analyzer-window/report.json")
const port = Number(process.argv[4] ?? 9237)
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Use a TCP port from 1024 to 65535")
const config = resolve(dirname(output), "config")
await mkdir(config, { recursive:true })
// Fail on an occupied port; never attach to a different app/profile by mistake.
try {
  await fetch(`http://127.0.0.1:${port}/json/version`, { signal:AbortSignal.timeout(500) })
  throw new Error(`DevTools port ${port} is already in use`)
} catch (error) {
  if (error.message.includes("already in use")) throw error
}
let logs = ""
const child = spawn(executable, [], {
  windowsHide:true,
  env:{ ...process.env, WINDFALL_ANALYZER_SMOKE:"1", WINDFALL_CONFIG_DIR:config,
    WINDFALL_SILENT:"0", WINDFALL_OUTPUT_GAIN:"0.01",
    WEBVIEW2_USER_DATA_FOLDER:resolve(config, "webview"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` },
  stdio:["ignore", "pipe", "pipe"],
})
child.stdout.on("data", (data) => { logs = (logs + data).slice(-16384) })
child.stderr.on("data", (data) => { logs = (logs + data).slice(-16384) })
let launchError
child.on("error", (error) => { launchError = error })
const report = { schema:1, executable, config, outputGain:0.01, started:new Date().toISOString(), fixture:null, samples:[], checks:[], passed:false }
let socket
try {
  let tab
  const deadline = Date.now() + 30000
  while (!tab && Date.now() < deadline) {
    if (launchError) throw launchError
    if (child.exitCode !== null) throw new Error(`App exited with ${child.exitCode}`)
    try {
      const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal:AbortSignal.timeout(1000) })).json()
      tab = tabs.find((tab) => tab.type === "page" && !tab.url.startsWith("devtools:") && tab.webSocketDebuggerUrl)
    } catch { /* WebView2 has not started yet. */ }
    if (!tab) await delay(100)
  }
  if (!tab) throw new Error("No inspectable app window within 30 seconds (use the built debug shell with bundled frontend)")
  socket = new WebSocket(tab.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("WebView2 DevTools websocket did not open within 5 seconds")), 5000)
    socket.addEventListener("open", () => { clearTimeout(timer); resolve() }, { once:true })
    socket.addEventListener("error", (error) => { clearTimeout(timer); reject(error) }, { once:true })
  })
  let next = 0
  const pending = new Map()
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data)
    const request = pending.get(message.id)
    if (!request) return
    pending.delete(message.id)
    clearTimeout(request.timer)
    if (message.error) request.reject(new Error(JSON.stringify(message.error)))
    else request.resolve(message.result)
  })
  const cdp = (method, params) => new Promise((resolve, reject) => {
    const id = ++next
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)) }, 15000)
    pending.set(id, { resolve, reject, timer })
    socket.send(JSON.stringify({ id, method, params }))
  })
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", { expression, awaitPromise:true, returnByValue:true })
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails))
    return result.result.value
  }
  const call = (method, ...args) => evaluate(`window.__windfallAnalyzerSmoke.${method}(...${JSON.stringify(args)})`)
  const until = async (predicate, what) => {
    const deadline = Date.now() + 10000
    while (Date.now() < deadline) {
      const found = await predicate()
      if (found) return found
      await delay(50)
    }
    throw new Error(`Timed out waiting for ${what}`)
  }
  await until(() => evaluate("Boolean(window.__windfallAnalyzerSmoke)"), "the explicitly enabled native smoke bridge")
  report.fixture = await call("prepare")
  await call("control", "play")
  const read = async (name) => {
    const display = await call("read")
    report.samples.push({ name, at:new Date().toISOString(), display })
    return display
  }
  const live = (track) => until(async () => {
    const view = await call("read")
    const source = view.statuses[0]?.source ? JSON.parse(view.statuses[0].source) : null
    return view.statuses[0]?.phase === "live" && view.canvases[0]?.live === "true" &&
      (track === undefined || (track === null ? source?.type === "mixOutput" : source?.track === track))
  }, "a current native analyzer frame in the real view")
  const check = (name, pass, details) => {
    report.checks.push({ name, pass:Boolean(pass), details })
    if (!pass) throw new Error(`Display acceptance failed: ${name}`)
  }
  await live(report.fixture.tracks[0])
  const left = await read("left")
  await delay(350)
  const advanced = await read("play-advanced")
  check("clock advances while playing", left.clock.time !== advanced.clock.time, [left.clock, advanced.clock])
  check("left source has only left level", Number(left.meters[0].peakDb) > -30 && left.meters[1].peakDb === "", left.meters)
  check("left spectrum peak near 440 Hz", Math.abs(Number(left.canvases[0].peakHz) - 440) <= Number(left.canvases[0].sampleRate) / 1024, left.canvases[0])
  await call("select", "right")
  await live(report.fixture.tracks[1])
  const right = await read("right")
  check("right source has only right level", right.meters[0].peakDb === "" && Number(right.meters[1].peakDb) > -30, right.meters)
  check("right spectrum peak near 1875 Hz", Math.abs(Number(right.canvases[0].peakHz) - 1875) <= Number(right.canvases[0].sampleRate) / 1024, right.canvases[0])
  const waves = right.waveforms
  check("per-track waveforms distinguish stereo sources", waves.length === 2 && Number(waves[0].leftPeak) > 0.05 && Number(waves[0].rightPeak) === 0 && Number(waves[1].leftPeak) === 0 && Number(waves[1].rightPeak) > 0.05, waves)
  await call("select", "mix")
  await live(null)
  const mix = await read("mix")
  check("mix includes both sources at nonzero quiet output gain", mix.meters.every((meter) => meter.peakDb !== ""), mix.meters)
  await call("control", "seek", 960)
  await delay(80)
  const seek = await read("seek-960")
  check("clock visibly follows seek", seek.clock.time !== mix.clock.time, seek.clock)
  await call("control", "loop")
  const loop = []
  for (let sample = 0; sample < 12; sample += 1) { await delay(100); loop.push((await read(`loop-${sample}`)).clock.time) }
  check("clock wraps inside the native loop", loop.some((time, index) => index > 0 && time < loop[index - 1]), loop)
  await call("select", "left")
  await live(report.fixture.tracks[0])
  await call("effect", "limiter")
  await until(async () => (await call("read")).limiter[0]?.blocks > 10, "native limiter history columns")
  const limiting = await read("limiter")
  check("limiter reports real input/output/reduction", Number(limiting.limiter[0].inputDb) > Number(limiting.limiter[0].outputDb) && Number(limiting.limiter[0].reductionDb) > 3, limiting.limiter)
  await call("control", "stop")
  await delay(750)
  const decay = await read("limiter-decay")
  check("limiter history moves and reduction releases", Number(decay.limiter[0].blocks) > Number(limiting.limiter[0].blocks) && Number(decay.limiter[0].lastReductionDb) < Number(limiting.limiter[0].lastReductionDb), decay.limiter)
  await call("effect", null)
  await delay(600)
  await call("control", "play")
  await call("effect", "tuner")
  await until(async () => (await call("read")).tuner.note === "A4", "calibrated tuner A4")
  const tuner = await read("tuner")
  check("tuner A4 and calibrated cents", tuner.tuner.note === "A4" && Math.abs(Number(tuner.tuner.values[0].cents) - 1200 * Math.log2(440 / 432)) < 2, tuner.tuner)
  await call("effect", null)
  await delay(600)
  await call("effect", "eq")
  await until(async () => Number((await call("read")).eq[0]?.peakHz) > 0, "actual EQ spectrum overlay")
  const eq = await read("eq")
  check("EQ live audio shares the response frequency axis", Math.abs(Number(eq.eq[0].peakHz) - 440) < 65, eq.eq)
  await call("effect", null)
  await delay(600)
  const beforeStall = await read("before-ui-stall")
  // Freeze the actual webview thread; no mock timer/channel stalls are used.
  await evaluate("(() => { const end = performance.now() + 2300; while (performance.now() < end) {} return true })()")
  await until(async () => {
    const view = await call("read")
    return view.statuses[0]?.phase === "live" && view.canvases[0]?.live === "true" && view.canvases[0]?.subscription !== beforeStall.canvases[0]?.subscription
  }, "a fresh native subscription after the actual webview stall")
  const recovered = await read("ui-recovery")
  check("UI stall recovers under a fresh lease and reports skipped events", Number(recovered.canvases[0].skippedEvents) > 0, recovered.canvases[0])
  await call("deleteSource")
  await until(async () => (await call("read")).statuses[0]?.reason === "sourceMissing", "deleted source refusal")
  // The refusal updates React before the animation-frame painters clear.
  // Wait for the existing display assertion, within the same deadline.
  await until(async () => {
    const view = await call("read")
    return view.canvases.every((canvas) => canvas.live === "false") &&
      view.meters.every((meter) => meter.peakDb === "")
  }, "deleted source display clearing on its animation frame")
  const gone = await read("deleted")
  check("source deletion clears real canvases and meters", gone.canvases.every((canvas) => canvas.live === "false") && gone.meters.every((meter) => meter.peakDb === ""), gone)
  await call("control", "replace")
  await delay(250)
  await read("project-replaced")
  report.passed = report.checks.every((check) => check.pass)
} catch (error) {
  report.error = String(error.stack ?? error)
  process.exitCode = 1
} finally {
  report.logs = logs
  report.finished = new Date().toISOString()
  socket?.close()
  child.kill()
  await mkdir(dirname(output), { recursive:true })
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`)
  console.log(`${report.passed ? "PASS" : "FAIL"}: ${output}`)
}
