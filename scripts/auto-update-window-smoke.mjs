// Node 22+, Windows, two signed NSIS installers of a test build. No packages.
//
// Proves that an installed Windfall updates itself, end to end, on this
// machine and without the real release host:
//
//   a throwaway per-user install of the older build in a folder of its own;
//   the app started on the software audio sink with its own config folder;
//   a local update host on loopback serving latest.json and the newer
//   installer; the start-up check finding the update by itself; packages
//   that must be refused (a tampered installer, a manifest signed with
//   another key, a genuine signature replayed under a higher version), each
//   leaving the old version running and installed; then Help → Check for
//   updates, Install now, a clean exit, the installer, and the relaunch as
//   the newer version. The install is removed afterwards.
//
// Both installers must be builds whose updater endpoint is
// http://127.0.0.1:UPDATE_PORT/latest.json and whose product name is
// "Windfall Update Test" (a separate identity, so the user's own Windfall
// install, its registry entries and shortcuts are never touched). See
// docs/qa/2026-10-14-auto-update.md for the build command.
//
//   node scripts/auto-update-window-smoke.mjs OLD_SETUP NEW_SETUP WORK_DIR [DEVTOOLS_PORT] [UPDATE_PORT]
import { spawn, spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { createServer } from "node:http"
import { copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { basename, dirname, join, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"
import { fileURLToPath } from "node:url"
import { main as writeManifest } from "./release/update-manifest.mjs"

const PRODUCT = "Windfall Update Test"
const oldSetup = resolve(process.argv[2] ?? "")
const newSetup = resolve(process.argv[3] ?? "")
const work = resolve(process.argv[4] ?? "target/autoupdate/e2e-run")
const port = Number(process.argv[5] ?? 9291)
const updatePort = Number(process.argv[6] ?? 18765)
const version = (name) => /_(\d+\.\d+\.\d+)_x64-setup\.exe$/.exec(basename(name))?.[1]
const oldVersion = version(oldSetup)
const newVersion = version(newSetup)
if (!oldVersion || !newVersion) throw new Error("Pass two NSIS installers named PRODUCT_VERSION_x64-setup.exe")
const repository = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const installDir = join(work, "install")
const config = join(work, "config")
const served = join(work, "served")
const exe = join(installDir, "windfall-desktop.exe")
const UNINSTALL_KEY = `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${PRODUCT}`
const LOCATION_KEY = `HKCU\\Software\\windfall\\${PRODUCT}`
// The user's own install, compared before and after.
const USER_KEYS = ["HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Windfall", "HKCU\\Software\\windfall\\Windfall"]
const USER_EXE = join(process.env.LOCALAPPDATA, "Windfall", "windfall-desktop.exe")

const report = { schema:1, oldSetup, newSetup, oldVersion, newVersion, installDir, config, audioBackend:"software-memory-sink", started:new Date().toISOString(), checks:[], requests:[], sessions:[], console:[], passed:false }
const check = (name, pass, detail) => {
  report.checks.push({ name, pass:Boolean(pass), detail })
  console.log(`${pass ? "PASS" : "FAIL"} ${name}${detail === undefined ? "" : ` ${JSON.stringify(detail).slice(0, 300)}`}`)
  if (!pass) throw new Error(`Failed: ${name}: ${JSON.stringify(detail)?.slice(0, 2000)}`)
}
const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex")
const exists = (path) => stat(path).then(() => true, () => false)
const powershell = (command) => {
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], { encoding:"utf8", windowsHide:true })
  return result.stdout.trim()
}
const regQuery = (key) => {
  const result = spawnSync("reg.exe", ["query", key, "/s"], { encoding:"utf8", windowsHide:true })
  return result.status === 0 ? result.stdout.replace(/\r/g, "").trim() : null
}
const regValue = (key, name) => /REG_SZ\s+(.*)$/m.exec(spawnSync("reg.exe", ["query", key, "/v", name], { encoding:"utf8", windowsHide:true }).stdout ?? "")?.[1]?.trim() ?? null
const fileVersion = (path) => powershell(`(Get-Item -LiteralPath '${path.replace(/'/g, "''")}').VersionInfo.ProductVersion`)
/** The app processes running from the throwaway install. */
const installedProcesses = () => {
  const text = powershell(`Get-CimInstance Win32_Process -Filter "Name='windfall-desktop.exe'" | ForEach-Object { "$($_.ProcessId)|$($_.ExecutablePath)" }`)
  return text.split(/\r?\n/).filter(Boolean).map((line) => { const [pid, path] = line.split("|"); return { pid:Number(pid), path } })
    .filter((process) => process.path?.toLowerCase() === exe.toLowerCase())
}
const alive = (pid) => powershell(`if (Get-Process -Id ${pid} -ErrorAction SilentlyContinue) { 'yes' }`) === "yes"
async function until(predicate, what, timeout = 15000) {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    const found = await predicate()
    if (found) return found
    await delay(100)
  }
  throw new Error(`Timed out waiting for ${what}`)
}

// --- The local update host. ---
let mode = "none"
const manifests = {}
const files = {}
function startServer() {
  const server = createServer(async (request, response) => {
    const entry = { at:new Date().toISOString(), mode, path:request.url, status:404, bytes:0 }
    report.requests.push(entry)
    try {
      let body = null
      if (request.url === "/latest.json" && manifests[mode]) body = Buffer.from(manifests[mode])
      else if (files[decodeURIComponent(request.url)]) body = await readFile(files[decodeURIComponent(request.url)])
      if (!body) { response.writeHead(404, { "Content-Length":0 }); response.end(); return }
      entry.status = 200; entry.bytes = body.length
      response.writeHead(200, { "Content-Type":request.url.endsWith(".json") ? "application/json" : "application/octet-stream", "Content-Length":body.length })
      response.end(body)
    } catch (error) { entry.status = 500; entry.error = String(error); response.writeHead(500); response.end() }
  })
  return new Promise((resolve, reject) => { server.once("error", reject); server.listen(updatePort, "127.0.0.1", () => resolve(server)) })
}

// --- The manifests: one genuine, three the app must refuse. ---
async function prepare() {
  await mkdir(served, { recursive:true })
  const name = basename(newSetup)
  const genuine = join(served, name)
  await copyFile(newSetup, genuine)
  await copyFile(`${newSetup}.sig`, `${genuine}.sig`)
  const notes = join(work, "notes.md")
  await writeFile(notes, `## Windfall ${newVersion} (update test)\n\n- A local build that proves automatic updates.\n- Served from loopback by the update smoke.\n`)
  // The release script verifies the real signature before it writes anything.
  const goodPath = join(served, "good.json")
  writeManifest(["--installer", genuine, "--version", newVersion, "--notes-file", notes, "--url", `http://127.0.0.1:${updatePort}/good/${encodeURIComponent(name)}`, "--output", goodPath])
  const good = JSON.parse(await readFile(goodPath, "utf8"))
  files[`/good/${name}`] = genuine
  manifests.good = JSON.stringify(good)

  const bytes = await readFile(genuine)
  const tamperedBytes = Buffer.from(bytes)
  tamperedBytes[Math.floor(tamperedBytes.length / 2)] ^= 0x40
  const tampered = join(served, "tampered.exe")
  await writeFile(tampered, tamperedBytes)
  files[`/tampered/${name}`] = tampered
  manifests.tampered = JSON.stringify({ ...good, platforms:{ "windows-x86_64":{ ...good.platforms["windows-x86_64"], url:`http://127.0.0.1:${updatePort}/tampered/${encodeURIComponent(name)}` } } })

  // Signed for the right version, with a key the app does not trust.
  const keys = join(work, "other-key")
  await mkdir(keys, { recursive:true })
  const cli = join(repository, "apps/desktop/node_modules/@tauri-apps/cli/tauri.js")
  const signer = (...args) => {
    const env = { ...process.env, CI:"true" }
    for (const name of ["TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PATH", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD"]) delete env[name]
    const result = spawnSync(process.execPath, [cli, "signer", ...args], { encoding:"utf8", windowsHide:true, env })
    if (result.status !== 0) throw new Error(`tauri signer ${args[0]} failed: ${result.stderr}`)
  }
  signer("generate", "--ci", "--password", "", "--write-keys", join(keys, "other.key"))
  const otherCopy = join(keys, name)
  await copyFile(genuine, otherCopy)
  signer("sign", "--private-key-path", join(keys, "other.key"), "--password", "", "--app-version", newVersion, otherCopy)
  const otherSignature = (await readFile(`${otherCopy}.sig`, "utf8")).trim()
  manifests["wrong-key"] = JSON.stringify({ ...good, platforms:{ "windows-x86_64":{ ...good.platforms["windows-x86_64"], signature:otherSignature } } })

  // The genuine package and signature, announced as a later version.
  const [major, minor, patch] = newVersion.split(".").map(Number)
  manifests.replay = JSON.stringify({ ...good, version:`${major}.${minor}.${patch + 1}` })
  return { name, genuine, good }
}

// --- WebView2 DevTools: attach to whichever process is on the port. ---
async function attach(label, pid) {
  const session = { label, pid, console:[] }
  report.sessions.push(session)
  let tab
  await until(async () => {
    if (pid !== undefined && !alive(pid)) throw new Error(`${label}: the app exited before its window appeared`)
    try {
      const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal:AbortSignal.timeout(1000) })).json()
      tab = tabs.find((tab) => tab.type === "page" && !tab.url.startsWith("devtools:") && tab.webSocketDebuggerUrl)
    } catch { /* Not up yet. */ }
    return tab
  }, `${label}: an inspectable window`, 90000)
  const socket = new WebSocket(tab.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, { once:true }); socket.addEventListener("error", reject, { once:true }) })
  let next = 0
  const pending = new Map()
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data)
    if (message.method === "Runtime.consoleAPICalled" && ["error", "assert", "warning", "info"].includes(message.params.type))
      session.console.push({ type:message.params.type, text:message.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" ").slice(0, 2000) })
    if (message.method === "Runtime.exceptionThrown")
      session.console.push({ type:"exception", text:`${message.params.exceptionDetails.text} ${message.params.exceptionDetails.exception?.description ?? ""}`.slice(0, 2000) })
    const request = pending.get(message.id)
    if (!request) return
    pending.delete(message.id)
    if (message.error) request.reject(new Error(JSON.stringify(message.error)))
    else request.resolve(message.result)
  })
  socket.addEventListener("close", () => { for (const request of pending.values()) request.reject(new Error("DevTools closed")); pending.clear() })
  const cdp = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++next
    pending.set(id, { resolve, reject })
    socket.send(JSON.stringify({ id, method, params }))
  })
  await cdp("Runtime.enable")
  const evaluate = async (expression) => {
    const result = await cdp("Runtime.evaluate", { expression, awaitPromise:true, returnByValue:true })
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails))
    return result.result.value
  }
  const wait = (expression, what, timeout) => until(() => evaluate(expression), what, timeout)
  await wait('typeof window.__TAURI_INTERNALS__?.invoke === "function" && Boolean(document.querySelector("[data-channel-button]"))', `${label}: the main window with its channel rack`, 90000)
  const invoke = (command, args = {}) => evaluate(`(async () => {
    const owned = await window.__TAURI_INTERNALS__.invoke("window_document_snapshot");
    return window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, { ...${JSON.stringify(args)}, caller: owned.caller });
  })()`)
  const appVersion = () => evaluate('window.__TAURI_INTERNALS__.invoke("plugin:app|version")')
  const visible = (selector, text) => `[...document.querySelectorAll(${JSON.stringify(selector)})].find((e) => ${text === undefined ? "true" : `e.textContent.trim().startsWith(${JSON.stringify(text)})`} && e.getClientRects().length > 0)`
  const click = async (selector, text) => {
    const point = await wait(`(() => {
      const element = ${visible(selector, text)}
      if (!element || element.disabled) return null
      const r = element.getBoundingClientRect(), x = r.x + r.width / 2, y = r.y + r.height / 2
      return element.contains(document.elementFromPoint(x, y)) ? { x, y } : null
    })()`, `${label}: ${selector} ${text ?? ""}`)
    await cdp("Input.dispatchMouseEvent", { type:"mouseMoved", x:point.x, y:point.y })
    await cdp("Input.dispatchMouseEvent", { type:"mousePressed", x:point.x, y:point.y, button:"left", buttons:1, clickCount:1 })
    await cdp("Input.dispatchMouseEvent", { type:"mouseReleased", x:point.x, y:point.y, button:"left", buttons:0, clickCount:1 })
  }
  const menu = async (title, item) => {
    await click('[role="menubar"] [role="menuitem"]', title)
    await click('[role="menu"] [role="menuitem"]', item)
  }
  const NOTICE = 'section[aria-label="Update available"]'
  const notice = () => evaluate(`(() => { const n = ${visible(NOTICE)}; return n ? { heading:n.querySelector("h3")?.textContent ?? "", text:n.innerText, alert:n.querySelector('[role="alert"]')?.textContent ?? "" } : null })()`)
  const close = () => { try { socket.close() } catch { /* Gone with the process. */ } }
  return { session, evaluate, wait, invoke, appVersion, click, menu, notice, NOTICE, close }
}

/** One refused package: the notice raises it, Install now is refused, nothing changes. */
async function refused(app, pid, scenario, expected, how) {
  mode = scenario
  const before = report.requests.length
  if (how === "menu") await app.menu("Help", "Check for updates")
  const shown = await until(async () => { const n = await app.notice(); return n?.heading.includes(expected) && !n.alert ? n : null }, `${scenario}: the notice for ${expected}`, 45000)
  check(`${scenario}: ${how === "menu" ? "Help → Check for updates" : "the start-up check"} raises the notice`, true, shown.heading)
  await app.click(`${app.NOTICE} button`, "Install now")
  const failure = await until(async () => (await app.notice())?.alert, `${scenario}: the refusal`, 60000)
  const requests = report.requests.slice(before)
  check(`${scenario}: the package is downloaded and refused before anything runs`, /verification failed|Nothing was installed/i.test(failure) && requests.some((r) => r.path !== "/latest.json" && r.status === 200), { failure, requests:requests.map((r) => `${r.path} ${r.status} ${r.bytes}`) })
  await delay(1500)
  check(`${scenario}: ${oldVersion} keeps running and stays installed`, alive(pid) && await app.appVersion() === oldVersion && regValue(UNINSTALL_KEY, "DisplayVersion") === oldVersion && await sha256(exe) === report.installedSha256, { alive:alive(pid) })
  const engine = await app.invoke("engine_status")
  check(`${scenario}: audio is back on the software sink after the refusal`, engine.host === "Software" && engine.running, engine)
  await app.click(`${app.NOTICE} button`, "Later")
  await until(async () => !(await app.notice()), `${scenario}: Later to put the notice away`)
  return failure
}

let server
let launched
let failure
try {
  // --- Preflight: nothing of a previous run, the user's install noted. ---
  check("no earlier test install is registered", regQuery(UNINSTALL_KEY) === null, regQuery(UNINSTALL_KEY))
  check("no app is running from the test install folder", installedProcesses().length === 0)
  report.userKeys = Object.fromEntries(USER_KEYS.map((key) => [key, regQuery(key)]))
  report.userExe = await exists(USER_EXE) ? await sha256(USER_EXE) : null
  await rm(work, { recursive:true, force:true })
  await mkdir(config, { recursive:true })
  const { name } = await prepare()
  server = await startServer()

  // --- Install the old version, silently, per user, in the throwaway folder. ---
  const oldCopy = join(work, "old-setup.exe")
  await copyFile(oldSetup, oldCopy)
  const install = spawnSync(oldCopy, ["/S", "/NS", `/D=${installDir}`], { windowsVerbatimArguments:true, windowsHide:true, timeout:180000 })
  check(`the ${oldVersion} installer installs into the test folder`, install.status === 0 && await exists(exe) && regValue(UNINSTALL_KEY, "DisplayVersion") === oldVersion, { status:install.status, location:regValue(UNINSTALL_KEY, "InstallLocation") })
  report.installedSha256 = await sha256(exe)
  check(`the installed executable is version ${oldVersion}`, fileVersion(exe) === oldVersion, fileVersion(exe))

  // --- Start it like the smokes do: software sink, its own config and WebView2 data. ---
  mode = "tampered"
  const started = performance.now()
  const startedAt = Date.now()
  launched = spawn(exe, [], {
    windowsHide:true,
    env:{ ...process.env, WINDFALL_SOFTWARE_AUDIO:"1", WINDFALL_CONFIG_DIR:config, WINDFALL_SILENT:"1",
      WEBVIEW2_USER_DATA_FOLDER:join(config, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` },
    stdio:"ignore",
  })
  let exited = null
  launched.on("exit", (code) => { exited = { code, at:performance.now() } })
  const old = await attach(`${oldVersion}`, launched.pid)
  old.session.startupMs = Math.round(performance.now() - started)
  check(`${oldVersion} starts to an interactive window`, await old.appVersion() === oldVersion, { startupMs:old.session.startupMs })
  const engine = await old.invoke("engine_status")
  check("it runs on the software sink, not an audio device", engine.host === "Software" && engine.device === "Memory sink (no audio device)", engine)
  check("no update request before the start-up delay", report.requests.length === 0, report.requests)

  // --- The start-up check, by itself, finds the update; the tampered package is refused. ---
  const firstRequest = await until(() => report.requests.find((r) => r.path === "/latest.json"), "the start-up check", 45000)
  report.startupCheckAfterMs = Date.parse(firstRequest.at) - startedAt
  check("the start-up check runs by itself a few seconds after start-up", report.startupCheckAfterMs >= 5000, { afterMs:report.startupCheckAfterMs })
  await refused(old, launched.pid, "tampered", `Windfall ${newVersion} is available`, "start-up")
  await refused(old, launched.pid, "wrong-key", `Windfall ${newVersion} is available`, "menu")
  const [major, minor, patch] = newVersion.split(".").map(Number)
  await refused(old, launched.pid, "replay", `Windfall ${major}.${minor}.${patch + 1} is available`, "menu")

  // --- No manifest on the host (what GitHub answers before a release has one): a clear failure. ---
  mode = "missing"
  await old.menu("Help", "Check for updates")
  const said = await until(() => old.evaluate('[...document.querySelectorAll("[data-sonner-toast]")].map((t) => t.innerText).find((t) => t.includes("Could not check for updates")) ?? null'), "the failed check's message", 30000)
  check("a manual check with no manifest on the host says so clearly", /Update check failed; retry when online\..*404/s.test(said) && !(await old.notice()) && alive(launched.pid), said)

  // --- The genuine update: Help → Check for updates, Install now, exit, install, relaunch. ---
  mode = "good"
  await old.menu("Help", "Check for updates")
  const shown = await until(async () => { const n = await old.notice(); return n?.heading.includes(`Windfall ${newVersion} is available`) && !n.alert ? n : null }, "the notice for the genuine update", 45000)
  check("the notice shows the version and the start of the release notes", shown.text.includes("A local build that proves automatic updates."), shown.text)
  const before = report.requests.length
  const clicked = performance.now()
  await old.click(`${old.NOTICE} button`, "Install now")
  // A project with unsaved edits would ask first; the new default project has none.
  const prompt = await until(async () => exited ? "exited" : await old.evaluate('Boolean([...document.querySelectorAll(\'[role="dialog"]\')].find((d) => d.textContent.includes("Unsaved project")))').catch(() => false) ? "prompt" : null, "the app to exit for the installer", 120000)
  if (prompt === "prompt") { await old.click('[role="dialog"] button', "Discard and install") }
  await until(() => exited, `the ${oldVersion} process to exit`, 120000)
  old.close()
  report.exitAfterInstallNowMs = Math.round(exited.at - clicked)
  const download = report.requests.slice(before).find((r) => r.path === `/good/${encodeURIComponent(name)}`)
  check(`${oldVersion} downloads the whole genuine installer once, then exits cleanly`, exited.code === 0 && download?.status === 200 && download.bytes === (await stat(newSetup)).size, { exit:exited.code, download, afterMs:report.exitAfterInstallNowMs, prompt })
  const relaunch = await until(() => installedProcesses()[0], "the installer to start the new version", 240000)
  report.relaunchAfterExitMs = Math.round(performance.now() - exited.at)
  const updated = await attach(`${newVersion}`, relaunch.pid)
  check(`the installer relaunches the app as ${newVersion}`, await updated.appVersion() === newVersion && regValue(UNINSTALL_KEY, "DisplayVersion") === newVersion && fileVersion(exe) === newVersion, { pid:relaunch.pid, path:relaunch.path, relaunchAfterExitMs:report.relaunchAfterExitMs })
  check("the update replaced the installed executable in place", await sha256(exe) !== report.installedSha256 && regValue(UNINSTALL_KEY, "InstallLocation")?.replace(/"/g, "") === installDir, regValue(UNINSTALL_KEY, "InstallLocation"))
  const after = await updated.invoke("engine_status")
  check("the relaunched app is still on the software sink with the test profile", after.host === "Software" && await exists(join(config, "webview")), after)
  await updated.menu("Help", "Check for updates")
  const current = await until(() => updated.evaluate('[...document.querySelectorAll("[data-sonner-toast]")].map((t) => t.innerText).find((t) => t.includes("You are up to date.")) ?? null'), "the up-to-date message", 30000)
  check("the relaunched app finds nothing newer", !(await updated.notice()), current)
  updated.close()
  powershell(`$p = Get-Process -Id ${relaunch.pid}; [void]$p.CloseMainWindow()`)
  await until(() => !alive(relaunch.pid), `the ${newVersion} app to quit`, 30000)
  check(`closing the ${newVersion} window quits it`, true)
  report.exceptions = report.sessions.flatMap((session) => session.console.filter((entry) => entry.type === "exception"))
  check("no uncaught exception in either session", report.exceptions.length === 0, report.exceptions)
  report.passed = true
} catch (error) {
  failure = error
  report.error = String(error.stack ?? error)
  console.error(error)
} finally {
  // --- Remove the test install and everything it left. ---
  for (const process of installedProcesses()) spawnSync("taskkill.exe", ["/PID", String(process.pid), "/T", "/F"], { windowsHide:true })
  if (launched && launched.exitCode === null) launched.kill()
  server?.close()
  if (await exists(join(installDir, "uninstall.exe"))) {
    spawnSync(join(installDir, "uninstall.exe"), ["/S"], { windowsHide:true, timeout:120000 })
    await until(async () => regQuery(UNINSTALL_KEY) === null && !(await exists(exe)), "the uninstaller", 120000).catch((error) => { report.cleanupError = String(error) })
  }
  spawnSync("reg.exe", ["delete", LOCATION_KEY, "/f"], { windowsHide:true })
  for (const folder of [join(process.env.LOCALAPPDATA, "org.windfall.WindfallUpdateTest"), join(process.env.APPDATA, "org.windfall.WindfallUpdateTest")]) await rm(folder, { recursive:true, force:true })
  for (const entry of await readdir(tmpdir()).catch(() => [])) if (entry.startsWith(`${PRODUCT}-`)) await rm(join(tmpdir(), entry), { recursive:true, force:true })
  report.cleanup = {
    uninstalled:regQuery(UNINSTALL_KEY) === null && regQuery(LOCATION_KEY) === null && !(await exists(exe)),
    shortcuts:["Microsoft\\Windows\\Start Menu\\Programs", "Microsoft\\Windows\\Start Menu\\Programs\\Windfall Update Test"].map((folder) => join(process.env.APPDATA, folder, `${PRODUCT}.lnk`)).concat(join(process.env.USERPROFILE, "Desktop", `${PRODUCT}.lnk`)),
    userKeysUnchanged:USER_KEYS.every((key) => regQuery(key) === report.userKeys[key]),
    userExeUnchanged:(await exists(USER_EXE) ? await sha256(USER_EXE) : null) === report.userExe,
  }
  report.cleanup.shortcutsLeft = (await Promise.all(report.cleanup.shortcuts.map(exists))).some(Boolean)
  const reportPath = join(dirname(work), `${basename(work)}-report.json`)
  report.finished = new Date().toISOString()
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`)
  await rm(work, { recursive:true, force:true }).catch((error) => { report.cleanupError = String(error) })
  console.log(`cleanup: ${JSON.stringify(report.cleanup)}`)
  console.log(`report: ${reportPath}`)
  if (failure || !report.cleanup.uninstalled || !report.cleanup.userKeysUnchanged || !report.cleanup.userExeUnchanged || report.cleanup.shortcutsLeft) process.exitCode = 1
}
