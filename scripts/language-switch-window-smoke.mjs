// Node 22+, Windows WebView2, prebuilt debug app. No packages or framework.
//
// Switches the interface language in the real desktop window and back, and
// checks that a menu, a dialog, an instrument editor, the history and a
// refusal from the backend are said in Spanish while it is Spanish, and that
// the project is exactly what it was. The Spanish it expects is read from the
// catalogs, so it never drifts from them.
import { spawn } from "node:child_process"
import { mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { dirname, join, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const executable = resolve(process.argv[2] ?? "target/debug/windfall-desktop.exe")
const output = resolve(process.argv[3] ?? "target/language-window/report.json")
const port = Number(process.argv[4] ?? 9275)
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Use a TCP port from 1024 to 65535")
const config = resolve(dirname(output), "config")
// A profile of its own, new each run: the language choice is kept in it.
await rm(config, { recursive:true, force:true })
await mkdir(config, { recursive:true })

// The Spanish entry of an English catalog key, from the catalog sources.
const catalogs = []
const collect = async (folder) => {
  for (const entry of await readdir(folder, { withFileTypes:true })) {
    const path = join(folder, entry.name)
    if (entry.isDirectory()) await collect(path)
    else if (entry.name.endsWith(".ts") && !entry.name.includes(".test.")) catalogs.push(await readFile(path, "utf8"))
  }
}
await collect(resolve("apps/desktop/src/lib/i18n"))
const es = (key) => {
  const line = new RegExp(`^\\s+${JSON.stringify(key).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}: ("(?:[^"\\\\]|\\\\.)*")`, "gm")
  const values = catalogs.flatMap((source) => [...source.matchAll(line)].map((match) => JSON.parse(match[1])))
  const spanish = values.find((value) => value !== key)
  if (!spanish) throw new Error(`No Spanish catalog entry for ${JSON.stringify(key)}`)
  return spanish
}

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
  env:{ ...process.env, WINDFALL_SOFTWARE_AUDIO:"1", WINDFALL_ANALYZER_SMOKE:"0", WINDFALL_CONFIG_DIR:config,
    WINDFALL_SILENT:"1", WINDFALL_OUTPUT_GAIN:"0",
    WEBVIEW2_USER_DATA_FOLDER:resolve(config, "webview"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` },
  stdio:["ignore", "pipe", "pipe"],
})
child.stdout.on("data", (data) => { logs = (logs + data).slice(-16384) })
child.stderr.on("data", (data) => { logs = (logs + data).slice(-16384) })
let launchError
child.on("error", (error) => { launchError = error })
const report = { schema:1, executable, config, audioBackend:"software-memory-sink", outputGain:0, started:new Date().toISOString(), checks:[], passed:false }
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
  const until = async (predicate, what) => {
    const deadline = Date.now() + 10000
    while (Date.now() < deadline) {
      const found = await predicate()
      if (found) return found
      await delay(50)
    }
    throw new Error(`Timed out waiting for ${what}`)
  }
  const invoke = (command, args = {}) => evaluate(`(async () => {
    const command = ${JSON.stringify(command)};
    const args = ${JSON.stringify(args)};
    if (command === "window_document_snapshot" || command === "document_snapshot") return window.__TAURI_INTERNALS__.invoke(command, args);
    const owned = await window.__TAURI_INTERNALS__.invoke("window_document_snapshot");
    return window.__TAURI_INTERNALS__.invoke(command, { ...args, caller: args.caller ?? owned.caller });
  })()`)
  const owned = () => invoke("window_document_snapshot")
  const dispatch = async (command) => invoke("dispatch", { command, generation:(await owned()).generation })
  const check = (name, pass, detail) => { report.checks.push({ name, pass:Boolean(pass), detail }); if (!pass) throw new Error(`Failed: ${name}: ${JSON.stringify(detail)}`) }
  // The centre of the first visible element under `selector` whose text starts with `text`.
  const point = (selector, text) => until(() => evaluate(`(() => {
    const element = [...document.querySelectorAll(${JSON.stringify(selector)})].find((e) =>
      (${text === undefined ? "true" : `e.textContent.trim().startsWith(${JSON.stringify(text)})`}) && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden")
    if (!element || element.disabled) return null
    element.scrollIntoView({ block:"center" })
    const r = element.getBoundingClientRect()
    return r.width && r.height ? { x:r.x + r.width / 2, y:r.y + r.height / 2 } : null
  })()`), `visible ${selector} ${text ?? ""}`)
  const click = async (selector, text) => {
    const p = await point(selector, text)
    await cdp("Input.dispatchMouseEvent", { type:"mouseMoved", ...p })
    await cdp("Input.dispatchMouseEvent", { type:"mousePressed", ...p, button:"left", clickCount:1 })
    await cdp("Input.dispatchMouseEvent", { type:"mouseReleased", ...p, button:"left", clickCount:1 })
  }
  const key = async (value, code = value, modifiers = 0) => {
    const windowsVirtualKeyCode = { a:65, Enter:13, Escape:27 }[value] ?? 0
    await cdp("Input.dispatchKeyEvent", { type:"keyDown", key:value, code, modifiers, windowsVirtualKeyCode })
    await cdp("Input.dispatchKeyEvent", { type:"keyUp", key:value, code, modifiers, windowsVirtualKeyCode })
  }
  const shown = (text) => evaluate(`document.body.innerText.includes(${JSON.stringify(text)})`)
  const texts = (selector) => evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].filter((e) => e.getClientRects().length > 0).map((e) => e.textContent.trim())`)
  const closed = (selector) => until(() => evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].every((e) => e.getClientRects().length === 0)`), `${selector} to close`)
  const language = () => evaluate("document.documentElement.lang")
  const menu = async (title, item) => {
    await click('[role="menubar"] [role="menuitem"]', title)
    if (item !== undefined) await click('[role="menu"] [role="menuitem"]', item)
  }
  const choose = async (name) => {
    await menu(await language() === "es" ? es("Options") : "Options", await language() === "es" ? es("Settings…") : "Settings…")
    const list = `[role="combobox"][aria-label=${JSON.stringify(await language() === "es" ? es("Interface language") : "Interface language")}]`
    // The dialog is still arriving when it is first clicked: open the list
    // until its options are there.
    for (let attempt = 0; ; attempt++) {
      await click(list)
      const deadline = Date.now() + 1500
      let open = false
      while (!open && Date.now() < deadline) {
        open = await evaluate(`[...document.querySelectorAll('[role="option"]')].some((e) => e.getClientRects().length > 0)`)
        if (!open) await delay(50)
      }
      if (open) break
      if (attempt === 4) throw new Error("The language list did not open")
    }
    await click('[role="option"]', name)
    await closed('[role="listbox"]')
  }

  await until(() => evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function" && Boolean(document.querySelector("[data-channel-button]"))'), "the real main desktop rack")
  // One edit to find in the history, and an instrument made from a descriptor.
  const created = await dispatch({ type:"addChannel", instrument:"tripleOsc", name:"Idioma" })
  const channel = created.created[0]
  const before = await owned()
  const project = JSON.stringify(before.snapshot.project)
  check("starts in English", await language() === "en" && (await texts('[role="menubar"] [role="menuitem"]')).includes("File"), await texts('[role="menubar"] [role="menuitem"]'))

  // English to Spanish, through the settings dialog as a user does it.
  await choose("Español")
  await until(async () => await language() === "es", "the interface to be Spanish")
  check("the settings dialog is Spanish and still open", await shown(es("Interface language")), await texts('[role="dialog"] h2, [role="dialog"] h3'))
  await key("Escape")
  await closed('[role="dialog"]')
  const menus = await texts('[role="menubar"] [role="menuitem"]')
  check("the menu bar is Spanish", [es("File"), es("Edit"), es("Options")].every((title) => menus.includes(title)) && !menus.includes("File"), menus)
  // The status bar says whether the project is on disk; this was English in
  // a Spanish window until this smoke was first run.
  check("the status bar is Spanish", await shown(es("Not saved to a file yet")) && !(await shown("Not saved to a file yet")), es("Not saved to a file yet"))

  await menu(es("File"))
  const items = await until(async () => { const found = await texts('[role="menu"] [role="menuitem"]'); return found.length ? found : null }, "the file menu")
  check("a menu is Spanish", [es("Save as template…"), es("Open project…")].every((title) => items.some((item) => item.startsWith(title))) && !items.some((item) => item.startsWith("Save as template")), items)

  // A dialog, and a refusal from the backend: a template may not take the
  // name of one that comes with the app. The backend sends a code and the name.
  const templates = await invoke("templates")
  const factory = templates.find((template) => template.factory).name
  await click('[role="menu"] [role="menuitem"]', es("Save as template…"))
  await until(() => shown(es("Save as template")), "the template prompt")
  check("a dialog is Spanish", await shown(es("Save as template")) && await shown(es("Name")), await texts('[role="dialog"], [role="alertdialog"]'))
  await click('[role="dialog"] input, [role="alertdialog"] input')
  await key("a", "KeyA", 2)
  await cdp("Input.insertText", { text:factory })
  await click('[role="dialog"] button, [role="alertdialog"] button', es("Save template"))
  const refusal = `«${factory}» es una plantilla incluida con Windfall. Elige otro nombre.`
  await until(() => shown(refusal), "the backend refusal in Spanish")
  check("a backend refusal is Spanish", await shown(refusal) && !(await shown("is a template that comes with Windfall")), refusal)
  if (await evaluate('Boolean(document.querySelector(\'[role="dialog"] input, [role="alertdialog"] input\'))')) { await key("Escape"); await closed('[role="dialog"] input, [role="alertdialog"] input') }

  // An instrument editor made from the descriptor of the new channel.
  await click(`[data-channel-button="${channel}"]`)
  await until(() => shown(es("Pulse width")), "the instrument editor in Spanish")
  check("an instrument editor is Spanish", await shown(es("Pulse width")) && await shown(es("Octave layer")) && !(await shown("Pulse width")), [es("Pulse width"), es("Octave layer")])

  // The history: the edit was made while the interface was English.
  await menu(es("Edit"), es("History"))
  await until(() => shown(es("Add channel")), "the history in Spanish")
  check("the history is Spanish", await shown(es("Add channel")) && await shown(es("Project opened")), es("Add channel"))
  await key("Escape")

  const during = await owned()
  check("the project is what it was while Spanish", JSON.stringify(during.snapshot.project) === project && during.generation === before.generation && during.snapshot.history.entries.length === before.snapshot.history.entries.length,
    { generation:[before.generation, during.generation], history:during.snapshot.history.entries.map((entry) => entry.label) })

  // And back.
  await choose("English")
  await until(async () => await language() === "en", "the interface to be English")
  await key("Escape")
  await closed('[role="dialog"]')
  const english = await texts('[role="menubar"] [role="menuitem"]')
  check("the menu bar is English again", ["File", "Edit", "Options"].every((title) => english.includes(title)) && !english.includes(es("File")), english)
  await menu("Edit", "History")
  await until(() => shown("Add channel"), "the history in English")
  check("the history is English again", await shown("Add channel") && !(await shown(es("Add channel"))), "Add channel")
  await key("Escape")
  const after = await owned()
  check("the project is intact after switching back", JSON.stringify(after.snapshot.project) === project && after.generation === before.generation && after.snapshot.dirty === before.snapshot.dirty,
    { generation:[before.generation, after.generation], channels:after.snapshot.project.channels.length })
  check("the instrument is still there", after.snapshot.project.channels.some((item) => item.id === channel && item.name === "Idioma"), channel)
  report.passed = report.checks.every((item) => item.pass)
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
  console.log(`${report.passed ? "PASS" : "FAIL"}: ${output} (${report.checks.length} checks)`)
}
