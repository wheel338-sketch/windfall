// Node 22+, Windows WebView2, a built app (release or debug), and the
// official MCP Inspector's command-line client. No packages of its own.
//
// Connects a real MCP client to the running app, over both transports, and
// checks what an agent can and cannot do:
//
//   the server turned on in Settings > AI agents (MCP) by clicks, and its
//   token read from the page; tools/list and get_project_summary over
//   Streamable HTTP with the token, in the legacy (initialize) and the
//   modern (2026-07-28) protocol eras, and over stdio through
//   `windfall-desktop --mcp-stdio` for each bridge executable given; a
//   pattern created and notes added by edit tools, each approved in the
//   window's approval panel by a click and each one undo step (Ctrl+Z takes
//   the notes back and leaves the pattern, Ctrl+Y brings them back); a
//   proposal made stale by a user edit and one rejected in the panel, both
//   refused afterwards to the user and to the agent; render_and_measure on a
//   short stretch with its WAV decoded here; requests without the token,
//   with a wrong token, a foreign Host or a foreign Origin refused; the
//   endpoint not reachable on another address; SoundFonts: a channel that
//   would be silent is warned about, flagged and listed by a render, a
//   SoundFont in a browser folder is found, its presets listed and one
//   loaded (approved in the panel, one undo step) so the channel sounds, a
//   channel added with a SoundFont in one call, and files outside the
//   browser folders refused; and the server turned off again, after which
//   both transports say so. The SoundFont is written here, the same small
//   General MIDI bank the session tests write (general_midi_fixture).
//
// The app runs on the software audio sink (no device is opened, nothing is
// heard) with a profile folder of its own. The Inspector is given as the
// path of its CLI entry point and runs from wherever it was installed.
//
//   node scripts/mcp-client-smoke.mjs APP_EXE INSPECTOR_CLI REPORT.json [BRIDGE_EXE ...]
//
// INSPECTOR_CLI is .../node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js.
// Each BRIDGE_EXE (APP_EXE when none is given) is run as the stdio bridge.
// Environment: MCP_SMOKE_DEVTOOLS_PORT (9291), MCP_SMOKE_PORT (47191), and
// MCP_SMOKE_INSPECTOR_NODE, the Node executable the Inspector runs on (this
// one by default; Node 26.4 on Windows aborts in libuv as the CLI exits).
import { spawn, spawnSync } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { request as httpRequest } from "node:http"
import { connect } from "node:net"
import { networkInterfaces } from "node:os"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const [appArg, inspectorArg, outputArg, ...bridgeArgs] = process.argv.slice(2)
if (!appArg || !inspectorArg || !outputArg) {
  console.error("usage: node scripts/mcp-client-smoke.mjs APP_EXE INSPECTOR_CLI REPORT.json [BRIDGE_EXE ...]")
  process.exit(2)
}
const executable = resolve(appArg)
const inspector = resolve(inspectorArg)
const output = resolve(outputArg)
const bridges = (bridgeArgs.length > 0 ? bridgeArgs : [appArg]).map((path) => resolve(path))
const devtools = Number(process.env.MCP_SMOKE_DEVTOOLS_PORT ?? 9291)
const mcpPort = Number(process.env.MCP_SMOKE_PORT ?? 47191)
const inspectorNode = resolve(process.env.MCP_SMOKE_INSPECTOR_NODE ?? process.execPath)
for (const port of [devtools, mcpPort]) if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Use TCP ports from 1024 to 65535")
for (const file of [executable, inspector, inspectorNode, ...bridges]) if (!existsSync(file)) throw new Error(`Not found: ${file}`)
const folder = dirname(output)
const config = resolve(folder, "config")
await rm(folder, { recursive:true, force:true })
await mkdir(config, { recursive:true })
const catalog = resolve(folder, "inspector-catalog.json")

// A one-preset General MIDI SoundFont ("Own GM", bank 0, program 0), byte
// for byte the session tests' general_midi_fixture.
function generalMidiFixture() {
  const le16 = (value) => { const bytes = Buffer.alloc(2); bytes.writeUInt16LE(value); return bytes }
  const le32 = (value) => { const bytes = Buffer.alloc(4); bytes.writeUInt32LE(value); return bytes }
  const chunk = (id, data) => Buffer.concat([Buffer.from(id, "latin1"), le32(data.length), data, Buffer.alloc(data.length % 2)])
  const list = (id, leaves) => chunk("LIST", Buffer.concat([Buffer.from(id, "latin1"), ...leaves.map(([tag, bytes]) => chunk(tag, bytes))]))
  const records = (pairs) => Buffer.concat(pairs.flatMap(([a, b]) => [le16(a), le16(b)]))
  const phdr = Buffer.alloc(76); phdr.write("Own GM", 0, "latin1"); phdr.write("EOP", 38, "latin1"); phdr.writeUInt16LE(1, 38 + 24)
  const inst = Buffer.alloc(44); inst.write("Tone", 0, "latin1"); inst.write("EOI", 22, "latin1"); inst.writeUInt16LE(1, 22 + 20)
  const shdr = Buffer.alloc(92); shdr.write("Wave", 0, "latin1"); shdr.write("EOS", 46, "latin1")
  shdr.writeUInt32LE(8192, 24); shdr.writeUInt32LE(128, 28); shdr.writeUInt32LE(4096, 32); shdr.writeUInt32LE(48000, 36); shdr[40] = 60; shdr.writeUInt16LE(1, 44)
  const smpl = Buffer.alloc((8192 + 46) * 2)
  for (let frame = 0; frame < 8192; frame++) smpl.writeInt16LE(6000, frame * 2)
  return chunk("RIFF", Buffer.concat([Buffer.from("sfbk", "latin1"),
    list("INFO", [["ifil", Buffer.from([2, 0, 4, 0])]]),
    list("sdta", [["smpl", smpl]]),
    list("pdta", [["phdr", phdr], ["pbag", records([[0, 0], [1, 0]])], ["pmod", Buffer.alloc(10)], ["pgen", records([[41, 0], [0, 0]])],
      ["inst", inst], ["ibag", records([[0, 0], [2, 0]])], ["imod", Buffer.alloc(10)], ["igen", records([[54, 1], [53, 0], [0, 0]])], ["shdr", shdr]]),
  ]))
}
const sounds = resolve(folder, "Sounds")
const bankPath = resolve(sounds, "Banks", "GM.sf2")
const secretPath = resolve(folder, "Private", "secret.sf2")
await mkdir(dirname(bankPath), { recursive:true })
await mkdir(dirname(secretPath), { recursive:true })
await writeFile(bankPath, generalMidiFixture())
await writeFile(secretPath, generalMidiFixture())
const endpoint = `http://127.0.0.1:${mcpPort}/mcp`

const report = { schema:1, executable, inspector, inspectorNode, bridges, config, endpoint, started:new Date().toISOString(), calls:[], checks:[], console:[], passed:false }
const check = (name, pass, detail) => {
  report.checks.push({ name, pass:Boolean(pass), detail })
  if (!pass) throw new Error(`Failed: ${name}: ${JSON.stringify(detail)?.slice(0, 2000)}`)
}
const soft = (name, pass, detail) => { report.checks.push({ name, pass:Boolean(pass), detail }) }

// --- The app, attached through WebView2's DevTools. ---
async function launch() {
  const session = { logs:"", console:[] }
  report.session = session
  const started = performance.now()
  const child = spawn(executable, [], {
    windowsHide:true,
    env:{ ...process.env, WINDFALL_SOFTWARE_AUDIO:"1", WINDFALL_CONFIG_DIR:config, WINDFALL_SILENT:"1",
      WEBVIEW2_USER_DATA_FOLDER:resolve(config, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-address=127.0.0.1 --remote-debugging-port=${devtools}` },
    stdio:["ignore", "pipe", "pipe"],
  })
  child.stdout.on("data", (data) => { session.logs = (session.logs + data).slice(-32768) })
  child.stderr.on("data", (data) => { session.logs = (session.logs + data).slice(-32768) })
  let launchError
  child.on("error", (error) => { launchError = error })
  let tab
  const deadline = Date.now() + 60000
  while (!tab && Date.now() < deadline) {
    if (launchError) throw launchError
    if (child.exitCode !== null) throw new Error(`App exited with ${child.exitCode}`)
    try {
      const tabs = await (await fetch(`http://127.0.0.1:${devtools}/json/list`, { signal:AbortSignal.timeout(1000) })).json()
      tab = tabs.find((tab) => tab.type === "page" && !tab.url.startsWith("devtools:") && tab.webSocketDebuggerUrl)
    } catch { /* WebView2 has not started yet. */ }
    if (!tab) await delay(50)
  }
  if (!tab) throw new Error("No inspectable app window within 60 seconds")
  const socket = new WebSocket(tab.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("WebView2 DevTools websocket did not open within 5 seconds")), 5000)
    socket.addEventListener("open", () => { clearTimeout(timer); resolve() }, { once:true })
    socket.addEventListener("error", (error) => { clearTimeout(timer); reject(error) }, { once:true })
  })
  let next = 0
  const pending = new Map()
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data)
    if (message.method === "Runtime.consoleAPICalled" && ["error", "assert", "warning"].includes(message.params.type)) {
      const text = message.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" ")
      session.console.push({ type:message.params.type, text:text.slice(0, 4000) })
    }
    if (message.method === "Runtime.exceptionThrown") {
      const details = message.params.exceptionDetails
      session.console.push({ type:"exception", text:`${details.text} ${details.exception?.description ?? ""}`.slice(0, 4000) })
    }
    if (message.method === "Log.entryAdded" && message.params.entry.level === "error") {
      session.console.push({ type:"log-error", text:`${message.params.entry.text} ${message.params.entry.url ?? ""}`.slice(0, 4000) })
    }
    const request = pending.get(message.id)
    if (!request) return
    pending.delete(message.id)
    clearTimeout(request.timer)
    if (message.error) request.reject(new Error(JSON.stringify(message.error)))
    else request.resolve(message.result)
  })
  const cdp = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++next
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)) }, 60000)
    pending.set(id, { resolve, reject, timer })
    socket.send(JSON.stringify({ id, method, params }))
  })
  await cdp("Runtime.enable")
  await cdp("Log.enable")
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", { expression, awaitPromise:true, returnByValue:true })
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails))
    return result.result.value
  }
  const until = async (predicate, what, timeout = 15000) => {
    const deadline = Date.now() + timeout
    while (Date.now() < deadline) {
      const found = await predicate()
      if (found) return found
      await delay(50)
    }
    throw new Error(`Timed out waiting for ${what}`)
  }
  await until(() => evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function" && Boolean(document.querySelector("[data-channel-button]"))'), "the main window with its channel rack", 60000)
  session.startupMs = Math.round(performance.now() - started)
  // Calls that need a command rather than a gesture; their checks say "by command".
  const invoke = (command, args = {}) => evaluate(`(async () => {
    const command = ${JSON.stringify(command)};
    const args = ${JSON.stringify(args)};
    if (command === "window_document_snapshot") return window.__TAURI_INTERNALS__.invoke(command, args);
    const owned = await window.__TAURI_INTERNALS__.invoke("window_document_snapshot");
    try { return { ok:await window.__TAURI_INTERNALS__.invoke(command, { ...args, caller: args.caller ?? owned.caller }) } }
    catch (error) { return { error:String(error?.message ?? error) } }
  })()`)
  const owned = () => invoke("window_document_snapshot")
  const visible = (selector, text, prefix) => `[...document.querySelectorAll(${JSON.stringify(selector)})].find((e) => (${text === undefined ? "true" : prefix ? `e.textContent.trim().startsWith(${JSON.stringify(text)})` : `e.textContent.trim() === ${JSON.stringify(text)}`}) && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden")`
  // The centre of the element, once it is the topmost thing there.
  const point = async (selector, text, prefix = false) => {
    let last = null
    return until(async () => {
      last = await evaluate(`(() => {
        const element = ${visible(selector, text, prefix)}
        if (!element || element.disabled || element.getAttribute("aria-disabled") === "true") return { missing:true }
        element.scrollIntoView({ block:"nearest", inline:"nearest" })
        const r = element.getBoundingClientRect()
        if (!r.width || !r.height) return { missing:true }
        const x = r.x + r.width / 2, y = r.y + r.height / 2
        const top = document.elementFromPoint(x, y)
        return element.contains(top) ? { x, y } : { covered:top ? top.outerHTML.slice(0, 200) : "nothing", x, y }
      })()`)
      return last.covered || last.missing ? null : last
    }, `visible ${selector} ${text ?? ""}`).catch((error) => { throw new Error(`${error.message} (${JSON.stringify(last)})`) })
  }
  const mouse = async (p) => {
    await cdp("Input.dispatchMouseEvent", { type:"mouseMoved", x:p.x, y:p.y })
    await cdp("Input.dispatchMouseEvent", { type:"mousePressed", x:p.x, y:p.y, button:"left", buttons:1, clickCount:1 })
    await cdp("Input.dispatchMouseEvent", { type:"mouseReleased", x:p.x, y:p.y, button:"left", buttons:0, clickCount:1 })
  }
  const click = async (selector, text, prefix) => mouse(await point(selector, text, prefix))
  const keyCodes = { a:65, z:90, y:89, Escape:27 }
  const key = async (value, code = value, modifiers = 0) => {
    const windowsVirtualKeyCode = keyCodes[value] ?? 0
    await cdp("Input.dispatchKeyEvent", { type:"rawKeyDown", key:value, code, modifiers, windowsVirtualKeyCode })
    await cdp("Input.dispatchKeyEvent", { type:"keyUp", key:value, code, modifiers, windowsVirtualKeyCode })
  }
  const menu = async (title, item) => {
    await click('[role="menubar"] [role="menuitem"]', title)
    await click('[role="menu"] [role="menuitem"]', item, true)
  }
  // Closes the window as the user does; the agent's edits are not saved, so
  // the app asks first and is told not to save.
  const quit = async () => {
    spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `$p = Get-Process -Id ${child.pid}; [void]$p.CloseMainWindow()`], { windowsHide:true })
    const deadline = Date.now() + 20000
    let asked = false
    while (child.exitCode === null && Date.now() < deadline) {
      if (!asked) {
        const prompt = await evaluate(`(() => { const b = [...document.querySelectorAll('[role="alertdialog"] button, [role="dialog"] button')].find((e) => e.textContent.trim() === "Don't save" && e.getClientRects().length > 0); if (!b) return null; const r = b.getBoundingClientRect(); return { x:r.x + r.width / 2, y:r.y + r.height / 2 } })()`).catch(() => null)
        if (prompt) { session.askedToSave = true; asked = true; await mouse(prompt).catch(() => {}) }
      }
      await delay(50)
    }
    socket.close()
    session.exitCode = child.exitCode
    if (child.exitCode === null) { child.kill(); throw new Error("The app did not exit within 20 s of closing its window") }
  }
  const kill = () => { try { socket.close() } catch {} ; if (child.exitCode === null) child.kill() }
  return { session, child, cdp, evaluate, until, invoke, owned, point, mouse, click, key, menu, quit, kill }
}

// --- The MCP Inspector's CLI, one process per call, as a user runs it. ---
function inspect(label, target, { method, tool, args = {}, era = "legacy", headers = [], env = {}, extra = [] }) {
  const argv = [inspector, "--method", method, "--protocol-era", era, "--connect-timeout", "20000"]
  if (tool) argv.push("--tool-name", tool)
  for (const [name, value] of Object.entries(args)) argv.push("--tool-arg", `${name}=${typeof value === "string" ? value : JSON.stringify(value)}`)
  for (const header of headers) argv.push("--header", header)
  for (const [name, value] of Object.entries(env)) argv.push("-e", `${name}=${value}`)
  argv.push(...extra)
  if (target.command) argv.push("--transport", "stdio", "--", target.command, ...target.args)
  else argv.push("--transport", "http", target.url)
  const started = performance.now()
  const run = spawnSync(inspectorNode, argv, {
    encoding:"utf8", windowsHide:true, timeout:180000, maxBuffer:64 * 1024 * 1024,
    env:{ ...process.env, MCP_CATALOG_PATH:catalog },
  })
  let json = null
  try { json = JSON.parse(run.stdout) } catch { /* not JSON: an error report. */ }
  const call = { label, method, tool, era, transport:target.command ? "stdio" : "http", status:run.status, ms:Math.round(performance.now() - started), stderr:run.stderr?.slice(0, 4000), stdout:json ? undefined : run.stdout?.slice(0, 4000) }
  report.calls.push(call)
  return { status:run.status, stdout:run.stdout, stderr:run.stderr ?? "", json }
}
// A tool call's structured result, or its text when it has none.
const structured = (result) => result?.structuredContent ?? (() => { try { return JSON.parse(result?.content?.[0]?.text) } catch { return null } })()
const failure = (result) => result?.isError === true ? result.content?.map((item) => item.text).join(" ") : null

// --- Raw HTTP, for what a client never sends. ---
function raw({ host = `127.0.0.1:${mcpPort}`, token, origin, method = "POST", body } = {}) {
  return new Promise((resolve, reject) => {
    const payload = body === undefined ? JSON.stringify({ jsonrpc:"2.0", id:1, method:"initialize", params:{ protocolVersion:"2025-11-25", capabilities:{}, clientInfo:{ name:"raw-smoke", version:"1" } } }) : body
    const headers = { Host:host, "Content-Type":"application/json", Accept:"application/json, text/event-stream" }
    if (token !== undefined) headers.Authorization = `Bearer ${token}`
    if (origin !== undefined) headers.Origin = origin
    if (method === "POST") headers["Content-Length"] = Buffer.byteLength(payload)
    const req = httpRequest({ host:"127.0.0.1", port:mcpPort, path:"/mcp", method, headers, timeout:10000 }, (res) => {
      let text = ""
      res.setEncoding("utf8")
      res.on("data", (chunk) => { text += chunk })
      res.on("end", () => resolve({ status:res.statusCode, headers:res.headers, body:text }))
    })
    req.on("error", reject)
    req.on("timeout", () => req.destroy(new Error("timeout")))
    if (method === "POST") req.write(payload)
    req.end()
  })
}
const reachable = (host, port) => new Promise((resolve) => {
  const socket = connect({ host, port, timeout:3000 })
  socket.once("connect", () => { socket.destroy(); resolve(true) })
  socket.once("error", () => resolve(false))
  socket.once("timeout", () => { socket.destroy(); resolve(false) })
})

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
  if (tag !== 3 || bits !== 32) throw new Error(`Expected 32-bit float, got format ${tag} with ${bits} bits`)
  const frames = Math.floor(data.length / (4 * channels))
  let peak = 0, sum = 0
  for (let at = 0; at + 4 <= frames * channels * 4; at += 4) {
    const value = data.readFloatLE(at)
    peak = Math.max(peak, Math.abs(value))
    sum += value * value
  }
  return { channels, rate, frames, peak, rms:Math.sqrt(sum / Math.max(1, frames * channels)) }
}
const dbfs = (level) => level > 0 ? 20 * Math.log10(level) : -120

const TOOLS = ["get_project_summary", "list_notes", "list_clips", "get_mixer_track", "get_parameters", "get_selection", "list_instruments_and_effects",
  "create_pattern", "add_notes", "remove_notes", "update_notes", "set_steps", "add_channel", "add_effect", "remove_effect", "move_effect", "set_parameter",
  "route_channel", "route_track", "set_send", "place_clips", "move_clips", "remove_clips", "set_tempo_meter", "rename_and_color", "write_automation",
  "apply_raw_commands", "get_proposal", "apply_proposal", "withdraw_proposal", "render_and_measure",
  "search_library", "list_soundfont_presets", "list_channel_presets", "load_soundfont_preset", "apply_channel_preset"]
const PANEL = 'aside[aria-label="AI agent proposals"]'
const card = (label) => `${PANEL} li[aria-label=${JSON.stringify(label)}]`

let app
try {
  app = await launch()
  report.startupMs = app.session.startupMs

  // --- Turning the server on, as a user does. ---
  check("the server is off by default: nothing listens on the port", !(await reachable("127.0.0.1", mcpPort)), { mcpPort })
  await app.menu("Options", "Settings…")
  await app.until(() => app.evaluate(`Boolean([...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Turn on the MCP server"))`), "the MCP section of Settings")
  await app.click("#mcp-port")
  await app.key("a", "KeyA", 2)
  await app.cdp("Input.insertText", { text:String(mcpPort) })
  await app.until(() => app.evaluate(`document.querySelector("#mcp-port").value === ${JSON.stringify(String(mcpPort))}`), "the port typed in")
  await app.click("button", "Turn on the MCP server")
  const shownUrl = await app.until(() => app.evaluate(`[...document.querySelectorAll('[role="status"]')].map((e) => e.textContent.trim()).find((text) => text.startsWith("Listening at")) ?? null`), "Listening at")
  check("Settings says where it listens", shownUrl.includes(endpoint), shownUrl)
  await app.click("button", "Show token")
  const token = await app.until(() => app.evaluate(`(() => { const text = document.querySelector('code[aria-label="Bearer token"]')?.textContent.trim() ?? ""; return /^[0-9a-f]{64}$/.test(text) ? text : null })()`), "the token shown")
  const saved = JSON.parse(await readFile(resolve(config, "mcp.json"), "utf8"))
  check("the settings file says on, the port chosen and the token shown", saved.enabled === true && saved.port === mcpPort && saved.token === token, { enabled:saved.enabled, port:saved.port, sameToken:saved.token === token })
  const snippet = await app.evaluate(`[...document.querySelectorAll("pre code")].map((e) => e.textContent).join("\\n")`)
  report.settingsSnippets = snippet
  check("the page shows the executable for the stdio bridge", snippet.includes("--mcp-stdio") && snippet.includes("windfall-desktop"), snippet.slice(0, 600))
  await app.key("Escape", "Escape")
  await app.until(() => app.evaluate(`![...document.querySelectorAll('[role="dialog"]')].some((e) => e.getClientRects().length > 0)`), "Settings to close")

  const http = { url:endpoint }
  const bearer = [`Authorization: Bearer ${token}`]

  // --- Read tools over HTTP, in both eras. ---
  for (const era of ["legacy", "modern"]) {
    const listed = inspect(`http ${era} tools/list`, http, { method:"tools/list", era, headers:bearer })
    const names = listed.json?.tools?.map((tool) => tool.name) ?? []
    check(`HTTP (${era}): tools/list gives the ${TOOLS.length} tools`, listed.status === 0 && names.length === TOOLS.length && TOOLS.every((name) => names.includes(name)), { status:listed.status, names, stderr:listed.stderr.slice(0, 800) })
    const summary = inspect(`http ${era} summary`, http, { method:"tools/call", tool:"get_project_summary", era, headers:bearer })
    const value = structured(summary.json)
    check(`HTTP (${era}): get_project_summary gives tempo, meter, channels and patterns`, summary.status === 0 && typeof value?.tempoBpm === "number" && Array.isArray(value?.channels) && value.channels.length > 0 && Array.isArray(value?.patterns), { status:summary.status, keys:value && Object.keys(value), stderr:summary.stderr.slice(0, 800) })
  }
  const auto = inspect("http auto tools/list", http, { method:"tools/list", era:"auto", headers:bearer })
  check("HTTP (auto): the client settles on an era and lists the tools", auto.status === 0 && auto.json?.tools?.length === TOOLS.length, { status:auto.status, stderr:auto.stderr.slice(0, 800) })
  const strict = inspect("http strict tools/list", http, { method:"tools/list", headers:bearer, extra:["--strict"] })
  check("the tool schemas pass the Inspector's strict portability check", strict.status === 0, { status:strict.status, stderr:strict.stderr.slice(0, 4000) })
  const resource = inspect("http resource", http, { method:"resources/read", headers:bearer, extra:["--uri", "windfall://project/summary"] })
  check("the summary is also a resource", resource.status === 0 && JSON.parse(resource.json?.contents?.[0]?.text ?? "null")?.tempoBpm > 0, { status:resource.status, stderr:resource.stderr.slice(0, 800) })

  const summary = structured(inspect("summary for ids", http, { method:"tools/call", tool:"get_project_summary", headers:bearer }).json)
  const channel = summary.channels[0]
  report.channel = channel

  // --- An edit, approved in the panel: a new pattern. ---
  const history = async () => (await app.owned()).snapshot.history
  const projectNow = async () => (await app.owned()).snapshot.project
  const before = await projectNow()
  const historyBefore = await history()
  const created = inspect("create_pattern", http, { method:"tools/call", tool:"create_pattern", headers:bearer, args:{ name:"Agent bass", lengthBars:1, title:"New pattern for the bass", explanation:"A one-bar pattern for the bass line." } })
  const proposal1 = structured(created.json)
  check("create_pattern files a pending proposal and names the pattern it will create", created.status === 0 && proposal1?.status?.state === "pending" && typeof proposal1.proposalId === "string" && proposal1.created?.patterns?.length === 1, { status:created.status, proposal1, failure:failure(created.json), stderr:created.stderr.slice(0, 800) })
  report.agentLabel = proposal1.label
  check("the undo label starts with the agent's name", /: New pattern for the bass$/.test(proposal1.label) && !proposal1.label.startsWith(":"), proposal1.label)
  check("nothing changed before the user decided", JSON.stringify(await projectNow()) === JSON.stringify(before), null)
  await app.until(() => app.evaluate(`Boolean(document.querySelector(${JSON.stringify(card(proposal1.label))}))`), "the proposal in the approval panel")
  report.panelText = await app.evaluate(`document.querySelector(${JSON.stringify(PANEL)}).innerText`)
  // How the panel sits in the window, kept for a person to look at.
  const shot = await app.cdp("Page.captureScreenshot", { format:"png" })
  await writeFile(resolve(folder, "approval-panel.png"), Buffer.from(shot.data, "base64"))
  report.panelBox = await app.evaluate(`(() => { const r = document.querySelector(${JSON.stringify(PANEL)}).getBoundingClientRect(); return { x:r.x, y:r.y, width:r.width, height:r.height, windowWidth:innerWidth, windowHeight:innerHeight } })()`)
  check("the panel lies inside the window", report.panelBox.x >= 0 && report.panelBox.y >= 0 && report.panelBox.x + report.panelBox.width <= report.panelBox.windowWidth && report.panelBox.y + report.panelBox.height <= report.panelBox.windowHeight, report.panelBox)
  check("the panel shows the agent's explanation and what changes", report.panelText.includes("A one-bar pattern for the bass line.") && /Patterns: \d+ → \d+/.test(report.panelText), report.panelText)
  await app.click(`${card(proposal1.label)} button`, "Apply")
  const patternId = proposal1.created.patterns[0]
  await app.until(async () => (await projectNow()).patterns.some((pattern) => pattern.id === patternId), "the new pattern")
  const historyAfter1 = await history()
  check("Apply makes the pattern as one undo step labelled with the agent's name", historyAfter1.cursor === historyBefore.cursor + 1 && historyAfter1.entries.at(historyAfter1.cursor - 1)?.label === proposal1.label, { before:historyBefore.cursor, after:historyAfter1.cursor, last:historyAfter1.entries.at(historyAfter1.cursor - 1) })
  const decided1 = structured(inspect("get_proposal applied", http, { method:"tools/call", tool:"get_proposal", headers:bearer, args:{ proposalId:proposal1.proposalId } }).json)
  check("the agent sees its proposal applied", decided1?.status?.state === "applied", decided1)

  // --- Notes, over the modern era, approved and undone as one step. ---
  const notes = [{ pitch:"C3", start:"1:1:0", length:"1/8" }, { pitch:"Eb3", start:"1:2:0", length:"1/8" }, { pitch:"G3", start:"1:3:0", length:"1/8" }, { pitch:"Bb3", start:"1:4:0", length:"1/8" }]
  const added = inspect("add_notes", http, { method:"tools/call", tool:"add_notes", era:"modern", headers:bearer, args:{ patternId, channelId:channel.id, notes, title:"Bass line", explanation:"Four notes of C minor seventh." } })
  const proposal2 = structured(added.json)
  check("add_notes (modern era) files a pending proposal", added.status === 0 && proposal2?.status?.state === "pending", { status:added.status, proposal2, failure:failure(added.json), stderr:added.stderr.slice(0, 800) })
  await app.click(`${card(proposal2.label)} button`, "Apply")
  const laneNotes = async () => (await projectNow()).patterns.find((pattern) => pattern.id === patternId)?.lanes.find((lane) => lane.channel === channel.id)?.notes ?? []
  await app.until(async () => (await laneNotes()).length === 4, "the four notes")
  const placed = (await laneNotes()).map((note) => [note.key, note.start, note.length])
  check("the notes are where and how long the agent said", JSON.stringify(placed) === JSON.stringify([[48, 0, 480], [51, 960, 480], [55, 1920, 480], [58, 2880, 480]]), placed)
  const historyAfter2 = await history()
  check("Apply adds the notes as one undo step", historyAfter2.cursor === historyAfter1.cursor + 1 && historyAfter2.entries.at(historyAfter2.cursor - 1)?.label === proposal2.label, { cursor:historyAfter2.cursor, last:historyAfter2.entries.at(historyAfter2.cursor - 1) })
  await app.evaluate("document.activeElement?.blur()")
  await app.key("z", "KeyZ", 2)
  await app.until(async () => (await history()).cursor === historyAfter1.cursor, "Ctrl+Z")
  check("one Ctrl+Z takes all four notes back and leaves the pattern", (await laneNotes()).length === 0 && (await projectNow()).patterns.some((pattern) => pattern.id === patternId), await laneNotes())
  await app.key("y", "KeyY", 2)
  await app.until(async () => (await laneNotes()).length === 4, "Ctrl+Y")
  check("Ctrl+Y brings the notes back", (await history()).cursor === historyAfter2.cursor, null)
  for (const decided of [proposal1, proposal2]) await app.click(`${card(decided.label)} button`, "Dismiss")

  // --- A stale proposal: the user edits the project first. ---
  const more = [{ pitch:"F3", start:"1:1:480", length:"1/16" }]
  const stale = structured(inspect("add_notes stale", http, { method:"tools/call", tool:"add_notes", headers:bearer, args:{ patternId, channelId:channel.id, notes:more, title:"Passing note" } }).json)
  check("a third proposal waits", stale?.status?.state === "pending", stale)
  await app.until(() => app.evaluate(`Boolean(document.querySelector(${JSON.stringify(card(stale.label))}))`), "the third proposal")
  const tempo = (await projectNow()).settings.tempoBpm
  const edit = await app.invoke("dispatch", { command:{ type:"updateSettings", patch:{ tempoBpm:tempo + 1 } }, generation:(await app.owned()).generation })
  check("the user changes the tempo (by command)", !edit.error, edit)
  await app.until(() => app.evaluate(`document.querySelector(${JSON.stringify(card(stale.label))})?.innerText.includes("The project changed since this was proposed")`), "the stale notice")
  const applyDisabled = await app.evaluate(`[...document.querySelectorAll(${JSON.stringify(`${card(stale.label)} button`)})].find((b) => b.textContent.trim() === "Apply")?.disabled === true`)
  check("the panel says the proposal is stale and Apply is disabled", applyDisabled, await app.evaluate(`document.querySelector(${JSON.stringify(card(stale.label))}).innerText`))
  const staleCursor = (await history()).cursor
  const staleSeen = structured(inspect("get_proposal stale", http, { method:"tools/call", tool:"get_proposal", headers:bearer, args:{ proposalId:stale.proposalId } }).json)
  check("the agent is told its proposal is stale", staleSeen?.stale === true && staleSeen?.status?.state === "pending", staleSeen)
  const staleApply = inspect("apply_proposal stale", http, { method:"tools/call", tool:"apply_proposal", headers:bearer, args:{ proposalId:stale.proposalId } })
  check("the agent cannot apply it itself", failure(staleApply.json) !== null && (await laneNotes()).length === 4, failure(staleApply.json))
  const forced = await app.invoke("mcp_approve", { id:stale.proposalId })
  check("applying the stale proposal anyway is refused (by command)", Boolean(forced.error) && (await laneNotes()).length === 4 && (await history()).cursor === staleCursor, forced)
  const staleAfter = structured(inspect("get_proposal stale after", http, { method:"tools/call", tool:"get_proposal", headers:bearer, args:{ proposalId:stale.proposalId } }).json)
  check("after the refused apply the agent sees it failed as stale", staleAfter?.status?.state === "failed" && /stale/.test(staleAfter.status.error), staleAfter)

  // --- A rejected proposal. ---
  const rejected = structured(inspect("add_notes rejected", http, { method:"tools/call", tool:"add_notes", headers:bearer, args:{ patternId, channelId:channel.id, notes:more, title:"Another passing note" } }).json)
  await app.until(() => app.evaluate(`Boolean(document.querySelector(${JSON.stringify(card(rejected.label))}))`), "the fourth proposal")
  const rejectCursor = (await history()).cursor
  await app.click(`${card(rejected.label)} button`, "Reject")
  await app.until(() => app.evaluate(`document.querySelector(${JSON.stringify(card(rejected.label))})?.innerText.includes("Rejected.")`), "Rejected.")
  const rejectedSeen = structured(inspect("get_proposal rejected", http, { method:"tools/call", tool:"get_proposal", headers:bearer, args:{ proposalId:rejected.proposalId, waitSeconds:2 } }).json)
  check("the agent is told its proposal was rejected", rejectedSeen?.status?.state === "rejected", rejectedSeen)
  const rejectedApply = inspect("apply_proposal rejected", http, { method:"tools/call", tool:"apply_proposal", headers:bearer, args:{ proposalId:rejected.proposalId } })
  const rejectedForced = await app.invoke("mcp_approve", { id:rejected.proposalId })
  check("a rejected proposal never applies, by the agent or by command", failure(rejectedApply.json) !== null && Boolean(rejectedForced.error) && (await laneNotes()).length === 4 && (await history()).cursor === rejectCursor, { agent:failure(rejectedApply.json), user:rejectedForced })

  // --- Listening: a short stretch, rendered and measured. ---
  const clipsBefore = (await projectNow()).playlist.clips.length
  const clipped = inspect("place_clips", http, { method:"tools/call", tool:"place_clips", headers:bearer, args:{ clips:[{ patternId, start:"1:1:0" }], newTrackName:"Agent bass", title:"Bass on the playlist" } })
  const proposal5 = structured(clipped.json)
  if (proposal5?.status?.state === "pending") {
    await app.click(`${card(proposal5.label)} button`, "Apply")
    await app.until(async () => (await projectNow()).playlist.clips.length === clipsBefore + 1, "the clip")
  }
  soft("place_clips puts the pattern on the playlist (approved in the panel)", proposal5?.status?.state === "pending", { proposal5, failure:failure(clipped.json) })
  const rendered = inspect("render_and_measure", http, { method:"tools/call", tool:"render_and_measure", headers:bearer, args:{ region:{ patternId, loops:2 }, maxSeconds:30 } })
  const listen = structured(rendered.json)
  const m = listen?.measurements
  report.measurements = m && { ...m, levelOverTime:m.levelOverTime?.length, octaveBands:m.octaveBands?.map((band) => [band.centerHz, band.relativeDb]) }
  check("render_and_measure renders the pattern twice and measures it", rendered.status === 0 && m && m.sampleRate === 48000 && m.seconds > 1 && m.seconds < 30, { status:rendered.status, failure:failure(rendered.json), stderr:rendered.stderr.slice(0, 800), seconds:m?.seconds })
  const expectedSeconds = 2 * 4 * 60 / (await projectNow()).settings.tempoBpm
  check("the render lasts two bars at the project's tempo", Math.abs(m.seconds - expectedSeconds) < 0.05, { seconds:m.seconds, expectedSeconds })
  check("the loudness and peaks are sane for four bass notes", Number.isFinite(m.integratedLufs) && m.integratedLufs > -60 && m.integratedLufs < 0 && m.samplePeakDbfs <= m.truePeakDbtp + 0.01 && m.truePeakDbtp < 3 && m.rmsDbfs < m.samplePeakDbfs && m.octaveBands.length === 10, report.measurements)
  const link = rendered.json?.content?.find((item) => item.type === "resource_link")
  check("the WAV is kept in Windfall's Agent renders folder and linked", typeof listen.wavPath === "string" && /Agent renders[\\/]Render-.*\.wav$/.test(listen.wavPath) && link?.uri?.startsWith("file:///"), { wavPath:listen.wavPath, link })
  const wav = decodeWav(await readFile(listen.wavPath))
  report.wav = wav
  check("the WAV decodes to the measured length and peak", wav.channels === 2 && wav.rate === 48000 && Math.abs(wav.frames / wav.rate - m.seconds) < 0.01 && Math.abs(dbfs(wav.peak) - m.samplePeakDbfs) < 0.1, { wav, measured:{ seconds:m.seconds, peak:m.samplePeakDbfs } })
  const tooLong = inspect("render too long", http, { method:"tools/call", tool:"render_and_measure", headers:bearer, args:{ region:{ patternId, loops:16 }, maxSeconds:1 } })
  check("a render longer than maxSeconds is refused before it starts", failure(tooLong.json)?.includes("maxSeconds"), failure(tooLong.json))

  // --- SoundFonts: a silent channel warned about and fixed by the agent. ---
  const call = (label, tool, args) => {
    const result = inspect(label, http, { method:"tools/call", tool, headers:bearer, args })
    return { ...result, value:structured(result.json), error:failure(result.json) }
  }
  const applyInPanel = async (proposal, what, done) => {
    await app.until(() => app.evaluate(`Boolean(document.querySelector(${JSON.stringify(card(proposal.label))}))`), `${what} in the panel`)
    await app.click(`${card(proposal.label)} button`, "Apply")
    await app.until(done, what)
  }
  const kinds = call("list_instruments_and_effects", "list_instruments_and_effects", { category:"instrument" }).value
  const soundFontKind = kinds?.items?.find((item) => item.kind === "soundFont")
  check("list_instruments_and_effects marks the SoundFont player as needing a source", soundFontKind?.needsSource === true && /silent until/.test(soundFontKind.description), soundFontKind)
  const channelsBefore = (await projectNow()).channels.length
  const choirAdd = call("add_channel choir", "add_channel", { instrument:"soundFont", name:"Choir", title:"Choir channel" })
  check("add_channel with an empty SoundFont player warns it will be silent", choirAdd.value?.status?.state === "pending" && /will be silent until it has a source/.test(choirAdd.value?.warning ?? ""), { value:choirAdd.value, error:choirAdd.error })
  await applyInPanel(choirAdd.value, "the Choir channel", async () => (await projectNow()).channels.length === channelsBefore + 1)
  const choirId = choirAdd.value.created.channels[0]
  const choirNotes = call("add_notes choir", "add_notes", { patternId, channelId:choirId, notes:[{ pitch:"C4", start:"1:1:0", length:"1/2" }], title:"Choir note" })
  await applyInPanel(choirNotes.value, "the Choir note", async () => (await projectNow()).patterns.find((pattern) => pattern.id === patternId)?.lanes.some((lane) => lane.channel === choirId && lane.notes.length === 1))
  const silentSummary = call("summary silent", "get_project_summary", {}).value
  const choirListed = silentSummary?.channels?.find((item) => item.id === choirId)
  check("get_project_summary flags the Choir as silent with what it needs", choirListed?.silent === true && /load_soundfont_preset/.test(choirListed.needs) && silentSummary.warnings.some((text) => text.includes("\"Choir\" is silent")), { choirListed, warnings:silentSummary?.warnings })
  check("the rack marks the Choir \"no SoundFont\" with a button that loads one", await app.evaluate(`document.querySelector('[data-load-source="${choirId}"]')?.textContent.trim() === "no SoundFont"`), null)
  const silentRender = call("render silent", "render_and_measure", { region:{ patternId }, maxSeconds:30 }).value
  const silentChoir = silentRender?.silentChannels?.find((item) => item.id === choirId)
  check("render_and_measure lists the Choir among the silent channels, with the reason", /no source loaded/.test(silentChoir?.reason ?? ""), silentRender?.silentChannels)
  // Where a SoundFont is: a folder of the browser (added by command, as the browser's + does).
  const rooted = await app.invoke("browser_add_root", { path:sounds })
  check("a folder with a SoundFont is added to the browser (by command)", !rooted.error, rooted)
  let found
  for (let attempt = 0; attempt < 40 && !(found?.total > 0); attempt++) {
    found = call("search_library", "search_library", { query:"GM", kind:"soundFont" }).value
    if (!(found?.total > 0)) await delay(250)
  }
  report.libraryFound = found
  check("search_library finds the SoundFont, with its kind and size", found?.total === 1 && found.items[0].kind === "soundFont" && found.items[0].path.endsWith("GM.sf2") && found.items[0].bytes > 16000, found)
  const libraryPath = found?.items?.[0]?.path ?? bankPath
  const presets = call("list_soundfont_presets", "list_soundfont_presets", { path:libraryPath }).value
  check("list_soundfont_presets gives the preset with its General MIDI family", presets?.items?.[0]?.name === "Own GM" && presets.items[0].family === "Piano" && presets.items[0].bank === 0 && presets.items[0].program === 0, presets)
  const cursorBeforeLoad = (await history()).cursor
  const loading = call("load_soundfont_preset", "load_soundfont_preset", { channelId:choirId, path:libraryPath, program:0, title:"Choir sound", explanation:"Give the Choir a SoundFont preset." })
  check("load_soundfont_preset files a pending proposal with no raw commands", loading.value?.status?.state === "pending" && loading.value.changes.some((text) => text.includes("\"Own GM\"")), { value:loading.value, error:loading.error })
  await app.until(() => app.evaluate(`Boolean(document.querySelector(${JSON.stringify(card(loading.value.label))}))`), "the SoundFont proposal")
  const loadText = await app.evaluate(`document.querySelector(${JSON.stringify(card(loading.value.label))}).innerText`)
  check("the panel says which preset of which file goes into which channel", loadText.includes('Loads the SoundFont preset "Own GM" (bank 0, program 0)') && loadText.includes("GM.sf2") && loadText.includes("Choir"), loadText)
  const choirSound = async () => (await projectNow()).channels.find((item) => item.id === choirId)?.sampleInstrument
  await applyInPanel(loading.value, "the SoundFont load", async () => (await choirSound())?.type === "soundFont")
  const afterLoad = await history()
  check("Apply loads the preset as one undo step labelled with the agent's name", afterLoad.cursor === cursorBeforeLoad + 1 && afterLoad.entries.at(afterLoad.cursor - 1)?.label === loading.value.label && (await choirSound())?.program === 0, { cursor:afterLoad.cursor, last:afterLoad.entries.at(afterLoad.cursor - 1), asset:await choirSound() })
  check("the rack no longer marks the Choir", await app.evaluate(`!document.querySelector('[data-load-source="${choirId}"]')`), null)
  const loudRender = call("render loaded", "render_and_measure", { region:{ patternId }, maxSeconds:30 }).value
  check("rendered again, the Choir is no longer silent", Array.isArray(loudRender?.silentChannels) && !loudRender.silentChannels.some((item) => item.id === choirId), loudRender?.silentChannels)
  // One call gives a playable channel.
  const horns = call("add_channel horns", "add_channel", { name:"Horns", soundFont:{ path:libraryPath, program:0 }, title:"Horns" })
  check("add_channel with soundFont files one proposal with no silence warning", horns.value?.status?.state === "pending" && !horns.value.warning, { value:horns.value, error:horns.error })
  const hornsId = horns.value?.created?.channels?.[0]
  await applyInPanel(horns.value, "the Horns channel", async () => (await projectNow()).channels.find((item) => item.id === hornsId)?.sampleInstrument?.type === "soundFont")
  check("the Horns channel arrives playing the preset", (await projectNow()).channels.find((item) => item.id === hornsId)?.name === "Horns", null)
  // Files outside the browser's folders.
  const foreign = call("presets outside", "list_soundfont_presets", { path:secretPath })
  check("a SoundFont outside the browser's folders is refused", /not inside/.test(foreign.error ?? ""), foreign.error)
  const climbing = call("presets climbing", "load_soundfont_preset", { channelId:choirId, path:`${sounds}\\..\\Private\\secret.sf2`, program:0 })
  check("a path that climbs out with .. is refused", /\.\./.test(climbing.error ?? ""), climbing.error)
  const rawLoad = call("raw soundfont", "apply_raw_commands", { commands:[{ type:"setSampleInstrumentAsset", channel:choirId, asset:{ type:"soundFont", path:{ kind:"external", path:secretPath }, bank:0, program:0 } }] })
  check("raw commands still cannot set a SoundFont", /registry/.test(rawLoad.error ?? ""), rawLoad.error)

  // --- What a client never sends. ---
  const good = await raw({ token })
  check("a request with the token is answered", good.status === 200 && good.headers["mcp-session-id"], { status:good.status })
  const none = await raw({})
  check("a request without the token is refused with 401", none.status === 401 && /Bearer/.test(none.headers["www-authenticate"] ?? ""), { status:none.status, headers:none.headers })
  const wrong = await raw({ token:"0".repeat(64) })
  check("a wrong token is refused with 401", wrong.status === 401, wrong.status)
  const foreignHost = await raw({ token, host:`evil.example:${mcpPort}` })
  check("a foreign Host is refused with 403 (DNS rebinding)", foreignHost.status === 403, foreignHost.status)
  const foreignOrigin = await raw({ token, origin:"http://evil.example" })
  check("a foreign Origin is refused with 403", foreignOrigin.status === 403, foreignOrigin.status)
  const ownOrigin = await raw({ token, origin:`http://127.0.0.1:${mcpPort}` })
  check("the endpoint's own Origin is let in", ownOrigin.status === 200, ownOrigin.status)
  const get = await raw({ token, method:"GET" })
  check("GET is not served (405)", get.status === 405, get.status)
  const noToken = inspect("http without token", http, { method:"tools/list" })
  check("the Inspector without the token is refused", noToken.status !== 0 && !noToken.json?.tools, { status:noToken.status, stderr:noToken.stderr.slice(0, 600) })
  const addresses = Object.values(networkInterfaces()).flat().filter((entry) => entry && entry.family === "IPv4" && !entry.internal).map((entry) => entry.address)
  const outside = []
  for (const address of addresses) outside.push({ address, reachable:await reachable(address, mcpPort) })
  check("the port is not reachable on the computer's other addresses", outside.every((entry) => !entry.reachable), outside)

  // --- stdio, through each bridge executable. ---
  for (const bridge of bridges) {
    const name = bridge.includes(`${"\\"}release${"\\"}`) || bridge.includes("/release/") || bridge.includes("stage") ? "release" : "debug"
    const stdio = { command:bridge, args:["--mcp-stdio", "--config-dir", config] }
    for (const era of ["legacy", "modern"]) {
      const listed = inspect(`stdio ${name} ${era} tools/list`, stdio, { method:"tools/list", era })
      check(`stdio (${name}, ${era}): tools/list gives the ${TOOLS.length} tools`, listed.status === 0 && listed.json?.tools?.length === TOOLS.length, { bridge, status:listed.status, stderr:listed.stderr.slice(0, 1200), stdout:listed.json ? undefined : listed.stdout.slice(0, 600) })
      const read = inspect(`stdio ${name} ${era} summary`, stdio, { method:"tools/call", tool:"get_project_summary", era })
      check(`stdio (${name}, ${era}): get_project_summary sees the agent's pattern`, structured(read.json)?.patterns?.some((pattern) => pattern.id === patternId), { status:read.status, stderr:read.stderr.slice(0, 800) })
    }
    // The environment variable names the folder, as the app's own does.
    const viaEnv = inspect(`stdio ${name} env`, { command:bridge, args:["--mcp-stdio"] }, { method:"tools/list", env:{ WINDFALL_CONFIG_DIR:config } })
    check(`stdio (${name}): WINDFALL_CONFIG_DIR finds the app too`, viaEnv.status === 0 && viaEnv.json?.tools?.length === TOOLS.length, { status:viaEnv.status, stderr:viaEnv.stderr.slice(0, 800) })
    const proposed = structured(inspect(`stdio ${name} propose`, stdio, { method:"tools/call", tool:"rename_and_color", args:{ target:"pattern", id:patternId, name:`Bass via ${name} bridge` } }).json)
    check(`stdio (${name}): an edit tool files a proposal`, proposed?.status?.state === "pending", proposed)
    await app.until(() => app.evaluate(`Boolean(document.querySelector(${JSON.stringify(card(proposed.label))}))`), "the stdio proposal in the panel")
    const withdrawn = structured(inspect(`stdio ${name} withdraw`, stdio, { method:"tools/call", tool:"withdraw_proposal", args:{ proposalId:proposed.proposalId } }).json)
    check(`stdio (${name}): the agent withdraws it`, withdrawn?.status?.state === "withdrawn", withdrawn)
    // The bridge's own errors reach the client's log on stderr, even from a
    // release (windows subsystem) executable.
    const off = spawnSync(bridge, ["--mcp-stdio", "--config-dir", resolve(folder, "no-such-config")], { encoding:"utf8", windowsHide:true, input:"", timeout:20000 })
    check(`stdio (${name}): with no server it exits 1 and says why on stderr`, off.status === 1 && /MCP server is off/.test(off.stderr), { status:off.status, stderr:off.stderr })
  }

  // --- Off again. ---
  await app.menu("Options", "Settings…")
  await app.click("button", "Turn off the MCP server")
  await app.until(() => app.evaluate(`Boolean([...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Turn on the MCP server"))`), "the server off")
  await app.key("Escape", "Escape")
  check("turned off, the port is closed", !(await reachable("127.0.0.1", mcpPort)), null)
  const offBridge = spawnSync(bridges[0], ["--mcp-stdio", "--config-dir", config], { encoding:"utf8", windowsHide:true, input:"", timeout:20000 })
  check("turned off, the bridge says the server is off", offBridge.status === 1 && /MCP server is off/.test(offBridge.stderr), { status:offBridge.status, stderr:offBridge.stderr })
  check("turned off, the settings file says so", JSON.parse(await readFile(resolve(config, "mcp.json"), "utf8")).enabled === false, null)

  await app.quit()
  check("closing the window asks to save the agent's edits, and quits cleanly without saving", app.session.askedToSave === true && app.session.exitCode === 0, { asked:app.session.askedToSave, exitCode:app.session.exitCode })
  const problems = app.session.console.filter((entry) => entry.type !== "warning")
  check("the webview console had no uncaught exception or error", problems.length === 0, problems)
  app = null
  report.passed = report.checks.every((item) => item.pass)
  if (!report.passed) process.exitCode = 1
} catch (error) {
  report.error = String(error.stack ?? error)
  report.windowText = await app?.evaluate("document.body.innerText").catch(() => null)
  process.exitCode = 1
} finally {
  app?.kill()
  report.console = report.session?.console ?? []
  report.logs = report.session?.logs?.slice(-8000)
  delete report.session
  report.finished = new Date().toISOString()
  await mkdir(folder, { recursive:true })
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`)
  console.log(`${report.passed ? "PASS" : "FAIL"}: ${output} (${report.checks.filter((item) => item.pass).length}/${report.checks.length} checks)`)
}
