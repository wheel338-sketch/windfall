// Node 22+, Windows WebView2, prebuilt debug app. No packages/framework.
// Authored RGB + stereo PCM AVI, real VideoPlayer, real native presentation
// clock/mixer. Run serially, after building the bundled debug shell.
import { spawn } from "node:child_process"
import { mkdir, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const executable = resolve(process.argv[2] ?? "target/debug/windfall-desktop.exe")
const output = resolve(process.argv[3] ?? "target/video-player-window/report.json")
const port = Number(process.argv[4] ?? 9241)
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Use a TCP port from 1024 to 65535")
if (process.platform !== "win32") throw new Error("This acceptance requires Windows WebView2")
const config = resolve(dirname(output), "config")
const fixtureDirectory = resolve(config, "video-smoke")
await mkdir(fixtureDirectory, { recursive:true })
const width = 4, height = 2, fps = 10, frames = 80, sampleRate = 48000
const chunk = (id, payload) => {
  const header = Buffer.alloc(8); header.write(id, 0, "ascii"); header.writeUInt32LE(payload.length, 4)
  return Buffer.concat([header, payload, Buffer.alloc(payload.length % 2)])
}
const list = (id, children) => chunk("LIST", Buffer.concat([Buffer.from(id), ...children]))
const stream = (kind, scale, rate, length, sampleSize) => {
  const header = Buffer.alloc(56); header.write(kind)
  header.writeUInt32LE(scale, 20); header.writeUInt32LE(rate, 24)
  header.writeUInt32LE(length, 32); header.writeUInt32LE(sampleSize, 44)
  return chunk("strh", header)
}
const dib = Buffer.alloc(40)
dib.writeUInt32LE(40); dib.writeInt32LE(width, 4); dib.writeInt32LE(height, 8)
dib.writeUInt16LE(1, 12); dib.writeUInt16LE(24, 14); dib.writeUInt32LE(width * height * 3, 20)
const wave = Buffer.alloc(16)
wave.writeUInt16LE(1); wave.writeUInt16LE(2, 2); wave.writeUInt32LE(sampleRate, 4)
wave.writeUInt32LE(sampleRate * 4, 8); wave.writeUInt16LE(4, 12); wave.writeUInt16LE(16, 14)
const avih = Buffer.alloc(56)
avih.writeUInt32LE(1e6 / fps); avih.writeUInt32LE(frames, 16)
avih.writeUInt32LE(2, 24); avih.writeUInt32LE(width, 32); avih.writeUInt32LE(height, 36)
const expectedPixel = (frame, x, y) => [frame + 1, 30 + x * 11 + y * 7, 200 - frame, 255]
const movi = [], index = []
let offset = 4
for (let frame = 0; frame < frames; frame += 1) {
  const rgb = Buffer.alloc(width * height * 3)
  for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) {
    const [r, g, b] = expectedPixel(frame, x, y)
    const at = ((height - y - 1) * width + x) * 3
    rgb[at] = b; rgb[at + 1] = g; rgb[at + 2] = r
  }
  const pcm = Buffer.alloc(sampleRate / fps * 4)
  // Every video frame has its own nonzero stereo amplitude. No other source
  // exists in the native project, so measured sound must come from this file.
  for (let at = 0; at < pcm.length; at += 4) {
    pcm.writeInt16LE(4096 + frame * 32, at); pcm.writeInt16LE(-8192 - frame * 32, at + 2)
  }
  for (const [id, payload] of [["00db", rgb], ["01wb", pcm]]) {
    const entry = Buffer.alloc(16); entry.write(id); entry.writeUInt32LE(0x10, 4)
    entry.writeUInt32LE(offset, 8); entry.writeUInt32LE(payload.length, 12); index.push(entry)
    const encoded = chunk(id, payload); movi.push(encoded); offset += encoded.length
  }
}
const bytes = chunk("RIFF", Buffer.concat([Buffer.from("AVI "),
  list("hdrl", [chunk("avih", avih),
    list("strl", [stream("vids", 1, fps, frames, 0), chunk("strf", dib)]),
    list("strl", [stream("auds", 4, sampleRate * 4, frames * sampleRate / fps, 4), chunk("strf", wave)])]),
  list("movi", movi), chunk("idx1", Buffer.concat(index))]))
await writeFile(resolve(fixtureDirectory, "authored-rgb-pcm.avi"), bytes)
try {
  await fetch(`http://127.0.0.1:${port}/json/version`, { signal:AbortSignal.timeout(500) })
  throw new Error(`DevTools port ${port} is already in use`)
} catch (error) { if (error.message.includes("already in use")) throw error }
let logs = "", launchError, socket
const child = spawn(executable, [], { windowsHide:true,
  env:{ ...process.env, WINDFALL_SOFTWARE_AUDIO:"1", WINDFALL_ANALYZER_SMOKE:"1", WINDFALL_CONFIG_DIR:config,
    WINDFALL_SILENT:"0", WINDFALL_OUTPUT_GAIN:"0.01",
    WEBVIEW2_USER_DATA_FOLDER:resolve(config, "webview"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` },
  stdio:["ignore", "pipe", "pipe"] })
child.stdout.on("data", (data) => { logs = (logs + data).slice(-16384) })
child.stderr.on("data", (data) => { logs = (logs + data).slice(-16384) })
child.on("error", (error) => { launchError = error })
const report = { schema:1, executable, config, audioBackend:"software-memory-sink", outputGain:0.01, authored:{ width, height, fps, frames, sampleRate, bytes:bytes.length },
  fixture:null, samples:[], checks:[], passed:false }
try {
  let tab
  const deadline = Date.now() + 30000
  while (!tab && Date.now() < deadline) {
    if (launchError) throw launchError
    if (child.exitCode !== null) throw new Error(`App exited with ${child.exitCode}`)
    try {
      const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal:AbortSignal.timeout(1000) })).json()
      tab = tabs.find((tab) => tab.type === "page" && !tab.url.startsWith("devtools:") && tab.webSocketDebuggerUrl)
    } catch { /* WebView2 startup. */ }
    if (!tab) await delay(100)
  }
  if (!tab) throw new Error("No inspectable app window within 30 seconds; use bundled debug shell")
  socket = new WebSocket(tab.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("DevTools websocket did not open")), 5000)
    socket.addEventListener("open", () => { clearTimeout(timer); resolve() }, { once:true })
    socket.addEventListener("error", (error) => { clearTimeout(timer); reject(error) }, { once:true })
  })
  let next = 0
  const pending = new Map()
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data), request = pending.get(message.id)
    if (!request) return
    pending.delete(message.id); clearTimeout(request.timer)
    if (message.error) request.reject(new Error(JSON.stringify(message.error)))
    else request.resolve(message.result)
  })
  const evaluate = async (expression) => {
    const result = await new Promise((resolve, reject) => {
      const id = ++next
      const timer = setTimeout(() => { pending.delete(id); reject(new Error("Runtime.evaluate timed out")) }, 15000)
      pending.set(id, { resolve, reject, timer })
      socket.send(JSON.stringify({ id, method:"Runtime.evaluate", params:{ expression, awaitPromise:true, returnByValue:true } }))
    })
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails))
    return result.result.value
  }
  const call = (method, ...args) => evaluate(`window.__windfallAnalyzerSmoke.${method}(...${JSON.stringify(args)})`)
  const until = async (predicate, name) => {
    const end = Date.now() + 10000
    while (Date.now() < end) {
      const result = await predicate()
      if (result) return result
      await delay(20)
    }
    report.timeoutDom = await evaluate("document.body.innerText.slice(0, 16000)")
    throw new Error(`Timed out: ${name}`)
  }
  const check = (name, pass, details) => {
    report.checks.push({ name, pass:Boolean(pass), details })
    if (!pass) throw new Error(`Video acceptance failed: ${name}`)
  }
  const read = async (name) => {
    const display = await call("read")
    report.samples.push({ name, display })
    return display
  }
  const painted = (view) => view.videos[0]?.playing === "true" && Number(view.videos[0].frameIndex) >= 0
  const pixelsAndTime = (name, view, offsetSeconds) => {
    const video = view.videos[0], tick = Number(video.tick), source = offsetSeconds + tick / 1920
    const frame = Math.floor(source * fps)
    const expected = []
    for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) expected.push(...expectedPixel(frame, x, y))
    check(`${name}: all real canvas pixels and frame time match native song clock`,
      painted(view) && video.width === width && video.height === height && Number(video.frameIndex) === frame &&
      Math.abs(Number(video.sourceSeconds) - source) < 1e-9 && video.epoch && video.outputFrame &&
      JSON.stringify(video.pixels) === JSON.stringify(expected), { video, source, frame })
  }
  const audioLive = async (name) => {
    const view = await until(async () => {
      const view = await call("read")
      return view.statuses[0]?.phase === "live" && view.meters.length === 2 && view.meters.every((meter) => meter.peakDb !== "" && Number(meter.peakDb) > -80) ? view : null
    }, "unmuted AVI soundtrack reaching native mixer/analyzer")
    report.samples.push({ name:`${name}-native-soundtrack`, display:view })
    check(`${name}: unmuted native soundtrack is playing with the picture`, painted(view), { video:view.videos[0], meters:view.meters, status:view.statuses[0] })
    return view
  }
  await until(() => evaluate("Boolean(window.__windfallAnalyzerSmoke)"), "opt-in native smoke host")
  report.fixture = await call("prepareVideo")
  check("native prepared and reopened an unmuted AVI placement", report.fixture.muted === false, report.fixture)
  await until(async () => (await call("read")).videos[0]?.frameIndex === "-1", "loaded canvas cleared before Play")
  await call("control", "play")
  await until(async () => { const view = await call("read"); return painted(view) ? view : null }, "first picture")
  const start = await read("start")
  pixelsAndTime("start", start, 0)
  check("start paints the beginning of the clip", Number(start.videos[0].sourceSeconds) < 0.25, start.videos[0])
  const sound = await audioLive("start")
  check("both native soundtrack channels are nonzero", sound.meters.length === 2 && sound.meters.every((meter) => meter.peakDb !== "" && Number(meter.peakDb) > -80), sound.meters)
  await call("control", "stop")
  await call("control", "videoOffset", 0.75)
  await call("control", "play")
  await until(async () => { const view = await call("read"); return painted(view) && Number(view.videos[0].sourceSeconds) >= 0.75 ? view : null }, "offset picture")
  const offsetView = await read("clip-offset")
  pixelsAndTime("offset", offsetView, 0.75)
  check("offset begins at requested source position", Number(offsetView.videos[0].sourceSeconds) < 1, offsetView.videos[0])
  await audioLive("clip-offset")
  const beforeSeekEpoch = offsetView.videos[0].epoch
  await call("control", "seek", 1920)
  await until(async () => { const view = await call("read"); return painted(view) && view.videos[0].epoch !== beforeSeekEpoch && Number(view.videos[0].tick) >= 1920 ? view : null }, "seek acknowledgement")
  const seek = await read("seek-1920")
  pixelsAndTime("seek", seek, 0.75)
  check("seek paints requested source frame", Number(seek.videos[0].sourceSeconds) >= 1.75 && Number(seek.videos[0].sourceSeconds) < 2, seek.videos[0])
  await audioLive("seek")
  await call("control", "loop") // actual native [960,1920) region, 500 ms.
  let last = null, wrapped = false
  for (let n = 0; n < 70 && !wrapped; n += 1) {
    await delay(20)
    const view = await read(`loop-${n}`), video = view.videos[0]
    if (!painted(view) || Number(video.tick) < 960 || Number(video.tick) >= 1920) continue
    pixelsAndTime("loop", view, 0.75)
    if (last && Number(video.tick) < Number(last.tick)) {
      check("loop wraps source frame with an advancing native output cursor",
        Number(video.frameIndex) < Number(last.frameIndex) && BigInt(video.outputFrame) > BigInt(last.outputFrame), [last, video])
      wrapped = true
    }
    last = video
  }
  check("observed a real native loop wrap", wrapped, last)
  await audioLive("loop")
  await call("control", "stop")
  await until(async () => { const view = await call("read"); return view.videos[0]?.playing === "false" && view.videos[0]?.frameIndex === "-1" ? view : null }, "Stop clearing the actual canvas")
  const stopped = await read("stop")
  check("Stop clears every RGBA pixel and source time", stopped.videos[0].pixels.every((value) => value === 0) && stopped.videos[0].sourceSeconds === "", stopped.videos[0])
  await until(async () => { const view = await call("read"); return view.meters.length === 2 && view.meters.every((meter) => meter.peakDb === "") }, "native soundtrack silence after Stop")
  await call("control", "play")
  await until(async () => painted(await call("read")), "picture resume after Stop")
  pixelsAndTime("resume", await read("resume"), 0.75)
  await audioLive("resume")
  report.passed = true
} catch (error) {
  report.error = error.stack ?? String(error)
  process.exitCode = 1
} finally {
  socket?.close(); child.kill()
  report.logs = logs; report.finished = new Date().toISOString()
  await mkdir(dirname(output), { recursive:true }); await writeFile(output, JSON.stringify(report, null, 2))
  console.log(JSON.stringify({ passed:report.passed, report:output, error:report.error }))
}
