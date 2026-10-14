// Node 22+, a built app (release or debug) on Linux or macOS. No packages.
//
// A launch smoke for platforms where the window cannot be driven through
// DevTools: starts the app on the software audio sink (no device is opened,
// nothing is heard) in a profile folder of its own, with its MCP server
// left on in that profile, and checks through the server over loopback:
//
//   the server answers at start-up; tools/list names the tools; the project
//   summary shows the default template with the factory kit loaded; the
//   window's frontend is running (it reports what the user is looking at,
//   which only the webview does); a proposal, approved in the window when
//   an approver is given (see below), so the song has notes; render_and_measure
//   over the song, its WAV decoded here; requests without the token refused;
//   and the app quits when asked.
//
//   node scripts/xplat/mcp-launch-smoke.mjs APP_EXECUTABLE REPORT.json [--approve osascript]
//
// --approve osascript presses the proposal's Apply button through macOS
// accessibility (System Events). Without it, or when accessibility is not
// permitted, the render covers the empty template and must be silent.
// Environment: MCP_SMOKE_PORT (47193). On Linux, DISPLAY must name an X
// server (Xvfb).
import { spawn, spawnSync } from "node:child_process"
import { randomBytes } from "node:crypto"
import { existsSync } from "node:fs"
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const args = process.argv.slice(2)
const approveAt = args.indexOf("--approve")
const approver = approveAt >= 0 ? args.splice(approveAt, 2)[1] : "none"
const [appArg, outputArg] = args
if (!appArg || !outputArg) {
  console.error("usage: node scripts/xplat/mcp-launch-smoke.mjs APP_EXECUTABLE REPORT.json [--approve osascript]")
  process.exit(2)
}
const executable = resolve(appArg)
const output = resolve(outputArg)
if (!existsSync(executable)) throw new Error(`Not found: ${executable}`)
const port = Number(process.env.MCP_SMOKE_PORT ?? 47193)
const folder = dirname(output)
const config = resolve(folder, "config")
await rm(folder, { recursive:true, force:true })
await mkdir(config, { recursive:true })
const token = randomBytes(32).toString("hex")
await writeFile(resolve(config, "mcp.json"), JSON.stringify({ enabled:true, port, token }))
const endpoint = `http://127.0.0.1:${port}/mcp`

const report = { schema:1, platform:process.platform, arch:process.arch, executable, config, endpoint, approver, started:new Date().toISOString(), checks:[], calls:[], passed:false }
const check = (name, pass, detail) => {
  report.checks.push({ name, pass:Boolean(pass), detail })
  if (!pass) throw new Error(`Failed: ${name}: ${JSON.stringify(detail)?.slice(0, 2000)}`)
}
const soft = (name, pass, detail) => { report.checks.push({ name, pass:Boolean(pass), detail, soft:true }) }

// --- JSON-RPC over Streamable HTTP, the legacy (initialize) era. ---
let session = null
let next = 0
async function post(body, { auth = true } = {}) {
  const headers = { "Content-Type":"application/json", Accept:"application/json, text/event-stream" }
  if (auth) headers.Authorization = `Bearer ${token}`
  if (session) { headers["Mcp-Session-Id"] = session; headers["MCP-Protocol-Version"] = "2025-06-18" }
  const response = await fetch(endpoint, { method:"POST", headers, body:JSON.stringify(body), signal:AbortSignal.timeout(180000) })
  const text = await response.text()
  return { status:response.status, headers:response.headers, json:text ? JSON.parse(text) : null }
}
async function rpc(method, params) {
  const started = performance.now()
  const answer = await post({ jsonrpc:"2.0", id:++next, method, params })
  report.calls.push({ method, tool:params?.name, status:answer.status, ms:Math.round(performance.now() - started), error:answer.json?.error ?? (answer.json?.result?.isError ? answer.json.result.content : undefined) })
  if (answer.status !== 200 || answer.json?.error) throw new Error(`${method}: ${answer.status} ${JSON.stringify(answer.json?.error ?? answer.json)}`)
  return answer.json.result
}
const tool = async (name, args = {}) => rpc("tools/call", { name, arguments:args })
const structured = (result) => result?.structuredContent ?? (() => { try { return JSON.parse(result?.content?.[0]?.text) } catch { return null } })()
const failure = (result) => result?.isError === true ? result.content?.map((item) => item.text).join(" ") : null

// --- The WAV, decoded here rather than trusted from the app. ---
function decodeWav(buffer) {
  if (buffer.toString("ascii", 0, 4) !== "RIFF" || buffer.toString("ascii", 8, 12) !== "WAVE") throw new Error("Not a RIFF/WAVE file")
  let format, data
  for (let at = 12; at + 8 <= buffer.length;) {
    const id = buffer.toString("ascii", at, at + 4), length = buffer.readUInt32LE(at + 4)
    if (id === "fmt ") format = buffer.subarray(at + 8, at + 8 + length)
    if (id === "data") data = buffer.subarray(at + 8, at + 8 + length)
    at += 8 + length + (length % 2)
  }
  if (!format || !data) throw new Error("WAV without fmt or data chunk")
  let tag = format.readUInt16LE(0)
  const channels = format.readUInt16LE(2), rate = format.readUInt32LE(4), bits = format.readUInt16LE(14)
  if (tag === 0xfffe) tag = format.readUInt16LE(24)
  const bytes = bits / 8, frames = Math.floor(data.length / (bytes * channels))
  const read = (at) => {
    if (tag === 3 && bits === 32) return data.readFloatLE(at)
    if (tag === 1 && bits === 16) return data.readInt16LE(at) / 32768
    if (tag === 1 && bits === 24) return data.readIntLE(at, 3) / 8388608
    if (tag === 1 && bits === 32) return data.readInt32LE(at) / 2147483648
    throw new Error(`Unsupported WAV encoding tag ${tag}, ${bits} bits`)
  }
  let peak = 0, nonFinite = 0
  for (let i = 0; i < frames * channels; i++) {
    const value = read(i * bytes)
    if (!Number.isFinite(value)) nonFinite++
    else peak = Math.max(peak, Math.abs(value))
  }
  return { tag, channels, rate, bits, frames, seconds:frames / rate, peak, nonFinite }
}

// Presses the Apply button of the proposal labelled `label` in the window.
function approve(pid, label) {
  if (approver !== "osascript") return { skipped:"no approver" }
  const script = `
on run argv
  set wanted to item 1 of argv
  set pid to (item 2 of argv) as integer
  tell application "System Events"
    set p to first process whose unix id is pid
    set frontmost of p to true
    repeat with e in (entire contents of window 1 of p)
      try
        if role of e is "AXButton" then
          set n to ""
          try
            set n to name of e
          end try
          if n is missing value then set n to ""
          set d to ""
          try
            set d to description of e
          end try
          if d is missing value then set d to ""
          if n is wanted or d is wanted then
            click e
            return "clicked"
          end if
        end if
      end try
    end repeat
  end tell
  return "missing"
end run`
  const result = spawnSync("osascript", ["-", "Apply", String(pid)], { input:script, encoding:"utf8", timeout:180000 })
  return { status:result.status, stdout:result.stdout?.trim(), stderr:result.stderr?.trim().slice(0, 2000), label }
}

// Lists the app's on-screen windows (owner, bounds) without screen-recording
// permission; titles are not needed.
function macWindows(pid) {
  const swift = `
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == Int32(CommandLine.arguments[1])! {
  let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
  print("\\(w[kCGWindowLayer as String] ?? -1) \\(b["Width"] ?? 0) \\(b["Height"] ?? 0)")
}`
  const result = spawnSync("swift", ["-", String(pid)], { input:swift, encoding:"utf8", timeout:120000 })
  return { status:result.status, windows:(result.stdout ?? "").trim().split("\n").filter(Boolean).map((line) => line.split(" ").map(Number)), stderr:result.stderr?.slice(0, 2000) }
}

function linuxWindows(pid) {
  const result = spawnSync("xdotool", ["search", "--onlyvisible", "--pid", String(pid)], { encoding:"utf8" })
  const windows = (result.stdout ?? "").trim().split("\n").filter(Boolean).map((id) => {
    const geometry = spawnSync("xdotool", ["getwindowgeometry", id], { encoding:"utf8" }).stdout ?? ""
    const size = /Geometry: (\d+)x(\d+)/.exec(geometry)
    return [0, Number(size?.[1] ?? 0), Number(size?.[2] ?? 0)]
  })
  return { status:result.status, windows }
}

function screenshot(path) {
  const result = process.platform === "darwin"
    ? spawnSync("screencapture", ["-x", path], { encoding:"utf8", timeout:30000 })
    : spawnSync("import", ["-window", "root", path], { encoding:"utf8", timeout:30000 })
  return { status:result.status, stderr:result.stderr?.slice(0, 500), path }
}

const started = performance.now()
const child = spawn(executable, [], {
  env:{ ...process.env, WINDFALL_SOFTWARE_AUDIO:"1", WINDFALL_CONFIG_DIR:config, WINDFALL_SILENT:"1", RUST_BACKTRACE:"1" },
  stdio:["ignore", "pipe", "pipe"],
})
let logs = ""
child.stdout.on("data", (data) => { logs = (logs + data).slice(-65536) })
child.stderr.on("data", (data) => { logs = (logs + data).slice(-65536) })
let launchError
child.on("error", (error) => { launchError = error })

try {
  // --- Start-up: the server answers. ---
  let initialized
  const deadline = Date.now() + 90000
  while (!initialized && Date.now() < deadline) {
    if (launchError) throw launchError
    if (child.exitCode !== null) throw new Error(`App exited with ${child.exitCode}`)
    try {
      const answer = await post({ jsonrpc:"2.0", id:++next, method:"initialize", params:{ protocolVersion:"2025-06-18", capabilities:{}, clientInfo:{ name:"xplat-smoke", version:"1" } } })
      if (answer.status === 200 && answer.json?.result) { initialized = answer; session = answer.headers.get("mcp-session-id") }
    } catch { /* Not listening yet. */ }
    if (!initialized) await delay(250)
  }
  check("the MCP server answers initialize over loopback at start-up", initialized, { logs:logs.slice(-4000) })
  report.startupMs = Math.round(performance.now() - started)
  report.server = initialized.json.result.serverInfo
  await post({ jsonrpc:"2.0", method:"notifications/initialized" })

  const unauthorized = await post({ jsonrpc:"2.0", id:++next, method:"tools/list" }, { auth:false })
  check("a request without the token is refused", unauthorized.status === 401, { status:unauthorized.status })

  const listed = await rpc("tools/list", {})
  const names = listed.tools.map((tool) => tool.name)
  check("tools/list names the read, edit and listen tools", ["get_project_summary", "add_notes", "render_and_measure"].every((name) => names.includes(name)), names)

  const summary = structured(await tool("get_project_summary"))
  report.summary = { tempoBpm:summary?.tempoBpm, channels:summary?.channels?.map((channel) => ({ id:channel.id, name:channel.name, source:channel.source })), patterns:summary?.patternCount, clips:summary?.playlist?.clipCount, samples:summary?.samples }
  check("the project summary shows the default template with its channels", summary?.tempoBpm > 0 && summary.channels?.length > 0, report.summary)
  check("the factory kit's samples are loaded into the template's channels", summary.samples?.length > 0 && summary.channels.some((channel) => channel.source?.type === "sampler" && channel.source.sample !== null), { channels:report.summary.channels, samples:summary.samples })

  // --- The frontend: it reports the view it shows a moment after it mounts. ---
  let selection
  const viewDeadline = Date.now() + 60000
  while (Date.now() < viewDeadline) {
    selection = structured(await tool("get_selection"))
    if (selection?.view) break
    await delay(500)
  }
  report.selection = selection
  check("the window's frontend is running and reports its view", typeof selection?.view === "string" && selection.view.length > 0, selection)

  // --- The window itself. ---
  const windows = process.platform === "darwin" ? macWindows(child.pid) : linuxWindows(child.pid)
  report.windows = windows
  check("the app has a main window of at least its minimum size on screen", windows.windows.some(([layer, width, height]) => layer === 0 && width >= 960 && height >= 600), windows)
  report.screenshot = screenshot(resolve(folder, "window.png"))

  // --- An edit through a proposal, approved in the window when possible. ---
  const pattern = selection.pattern?.id ?? summary.patterns?.[0]?.id
  const channel = summary.channels[0]
  const proposed = await tool("set_steps", { patternId:pattern, channelId:channel.id, steps:"x...x...x...x...", title:"Four on the floor", explanation:"Smoke test." })
  const proposal = structured(proposed)
  check("set_steps files a pending proposal", proposal?.status?.state === "pending", { proposal, failure:failure(proposed) })
  const approval = approve(child.pid, proposal.label)
  report.approval = approval
  let applied = false
  if (approver !== "none") {
    for (let i = 0; i < 20 && !applied; i++) {
      const state = structured(await tool("get_proposal", { proposalId:proposal.proposalId }))
      applied = state?.status?.state === "applied"
      if (!applied) await delay(500)
    }
    soft("the proposal was applied by pressing Apply in the window", applied, approval)
  }
  if (!applied) await tool("withdraw_proposal", { proposalId:proposal.proposalId })

  // --- Listening: the song (or the pattern when it is not on the playlist). ---
  const region = summary.playlist?.clipCount > 0 ? {} : { region:{ patternId:pattern, loops:1 } }
  const rendered = await tool("render_and_measure", { ...region, maxSeconds:30 })
  const listen = structured(rendered)
  report.render = { failure:failure(rendered), measurements:listen?.measurements ?? null, wavPath:listen?.wavPath }
  check("render_and_measure renders and keeps a WAV", !failure(rendered) && typeof listen?.wavPath === "string" && existsSync(listen.wavPath), report.render)
  const wav = decodeWav(await readFile(listen.wavPath))
  report.wav = wav
  check("the WAV decodes to finite stereo audio of some length", wav.channels === 2 && wav.frames > 0 && wav.nonFinite === 0, wav)
  if (applied) check("with the steps applied the render is audible", wav.peak > 0.01, wav)
  else check("without an approved edit the template renders silent", wav.peak < 1e-4, wav)

  // --- Quitting. ---
  child.kill("SIGTERM")
  const quitDeadline = Date.now() + 20000
  while (child.exitCode === null && child.signalCode === null && Date.now() < quitDeadline) await delay(100)
  check("the app exits when asked to terminate", child.exitCode !== null || child.signalCode !== null, { exitCode:child.exitCode, signal:child.signalCode })
  report.passed = report.checks.every((entry) => entry.pass || entry.soft)
} catch (error) {
  report.error = String(error?.stack ?? error)
  process.exitCode = 1
} finally {
  if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL")
  report.logs = logs.slice(-16000)
  report.finished = new Date().toISOString()
  await writeFile(output, JSON.stringify(report, null, 2))
  for (const entry of report.checks) console.log(`${entry.pass ? "PASS" : entry.soft ? "SOFT" : "FAIL"} ${entry.name}`)
  if (report.error) console.log(report.error)
  console.log(report.passed ? "PASSED" : "FAILED")
}
