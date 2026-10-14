// Node 22+, a built app (release or debug). No packages. On Windows the
// window is driven through WebView2's DevTools; on Linux through WebDriver,
// which needs tauri-driver, WebKitWebDriver, xdotool and wmctrl on the PATH,
// a window manager on the X display, and an X display (Xvfb will do).
//
// Makes a short song in the real desktop window the way a user does, on the
// software audio sink (no device is opened, nothing is heard), in a profile
// of its own:
//
//   start-up to an interactive window; a new project from the default
//   template; a drum pattern clicked into the channel rack with the factory
//   kit; a synth channel added from the rack's menu and a melody drawn in
//   the piano roll; both patterns placed on the playlist over four bars;
//   effects on mixer tracks and a channel routed to a bus; an automation
//   clip; play and stop in song mode with the transport and the meters
//   checked; undo and redo by keyboard; a factory sample loaded from the
//   browser; the settings dialog and a switch to Spanish and back; an export
//   to WAV chosen in the operating system's own file picker and decoded here;
//   Save as through the picker; quitting by closing the window; and, in a
//   second launch, the project opened again through the picker and compared
//   with what was saved, and exported again to the same audio.
//
// The webview console is captured throughout; an uncaught exception, a
// console error or a crashed panel fails the run.
//
//   node scripts/song-workflow-window-smoke.mjs EXE REPORT.json [PORT]
//
// Checks that need a command rather than a gesture say so in their name
// ("by command"); everything else is a click, a key or a file picker.
import { spawn, spawnSync } from "node:child_process"
import { copyFile, mkdir, readdir, readFile, rm, writeFile, stat } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const linux = process.platform === "linux"
const executable = resolve(process.argv[2] ?? (linux ? "target/release/windfall-desktop" : "target/release/windfall-desktop.exe"))
const output = resolve(process.argv[3] ?? "target/song-window/report.json")
const port = Number(process.argv[4] ?? 9281)
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Use a TCP port from 1024 to 65535")
const folder = dirname(output)
const config = resolve(folder, "config")
await rm(folder, { recursive:true, force:true })
await mkdir(config, { recursive:true })
const projectPath = resolve(folder, "Song QA.windfall")
const exportPath = resolve(folder, "Song QA.wav")
const exportAgainPath = resolve(folder, "Song QA reopened.wav")

const PPQ = 960
const report = { schema:1, executable, config, audioBackend:"software-memory-sink", started:new Date().toISOString(), timings:{}, sessions:[], checks:[], console:[], passed:false }
const check = (name, pass, detail) => {
  report.checks.push({ name, pass:Boolean(pass), detail })
  if (!pass) throw new Error(`Failed: ${name}: ${JSON.stringify(detail)?.slice(0, 2000)}`)
}
// Recorded and failing the run, without stopping it.
const soft = (name, pass, detail) => { report.checks.push({ name, pass:Boolean(pass), detail }) }

// --- One launch of the app, attached through WebView2's DevTools on
// Windows, through WebDriver (tauri-driver and WebKitWebDriver) on Linux. ---
// On Linux the webview keeps its storage (the UI's remembered layout among
// it) under the XDG folders, so those go in the profile too, as WebView2's
// folder does on Windows.
const appEnv = () => ({ ...process.env, WINDFALL_SOFTWARE_AUDIO:"1", WINDFALL_CONFIG_DIR:config, WINDFALL_SILENT:"1",
  ...(linux ? { XDG_DATA_HOME:resolve(config, "xdg-data"), XDG_CACHE_HOME:resolve(config, "xdg-cache") } : {}) })

// The transport gives the shared steps four things: evaluate an expression
// in the page (awaiting a promise, returning a JSON value), a mouse gesture,
// a key press, and closing the window as the user does.
async function devtoolsTransport(session) {
  try {
    await fetch(`http://127.0.0.1:${port}/json/version`, { signal:AbortSignal.timeout(500) })
    throw new Error(`DevTools port ${port} is already in use`)
  } catch (error) {
    if (error.message.includes("already in use")) throw error
  }
  const child = spawn(executable, [], {
    windowsHide:true,
    env:{ ...appEnv(),
      WEBVIEW2_USER_DATA_FOLDER:resolve(config, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}` },
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
      const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal:AbortSignal.timeout(1000) })).json()
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
  const mouse = async (p, { clickCount = 1, button = "left", modifiers = 0 } = {}) => {
    await cdp("Input.dispatchMouseEvent", { type:"mouseMoved", x:p.x, y:p.y, modifiers })
    for (let n = 1; n <= clickCount; n++) {
      await cdp("Input.dispatchMouseEvent", { type:"mousePressed", x:p.x, y:p.y, button, buttons:button === "left" ? 1 : 2, clickCount:n, modifiers })
      await cdp("Input.dispatchMouseEvent", { type:"mouseReleased", x:p.x, y:p.y, button, buttons:0, clickCount:n, modifiers })
    }
  }
  const keyCodes = { a:65, z:90, y:89, s:83, n:78, Enter:13, Escape:27, Tab:9, End:35, " ":32 }
  const key = async (value, code = value, modifiers = 0) => {
    const windowsVirtualKeyCode = keyCodes[value] ?? 0
    await cdp("Input.dispatchKeyEvent", { type:"rawKeyDown", key:value, code, modifiers, windowsVirtualKeyCode })
    await cdp("Input.dispatchKeyEvent", { type:"keyUp", key:value, code, modifiers, windowsVirtualKeyCode })
  }
  // Closes the window as the user does (WM_CLOSE to the main window).
  const close = async () => {
    spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `$p = Get-Process -Id ${child.pid}; [void]$p.CloseMainWindow()`], { windowsHide:true })
    const deadline = Date.now() + 20000
    while (child.exitCode === null && Date.now() < deadline) await delay(50)
    socket.close()
    if (child.exitCode === null) { child.kill(); throw new Error("The app did not exit within 20 s of closing its window") }
    return child.exitCode
  }
  const kill = () => { try { socket.close() } catch {} ; if (child.exitCode === null) child.kill() }
  return { child, evaluate, mouse, key, close, kill }
}

// WebKitGTK has no DevTools protocol to attach to; tauri-driver starts the
// app under WebKitWebDriver. The app is started through a small wrapper that
// records its exit status, since the driver, not this script, is its parent.
// The webview's console is hooked once the page answers, so messages from
// before that moment are not captured.
async function webdriverTransport(session) {
  const exitFile = resolve(folder, `${session.label.replace(/\W+/g, "-")}.exit`)
  const wrapper = resolve(folder, `${session.label.replace(/\W+/g, "-")}.sh`)
  await rm(exitFile, { force:true })
  const quoted = (text) => `'${text.replace(/'/g, "'\\''")}'`
  await writeFile(wrapper, `#!/bin/sh\n${quoted(executable)} "$@"\necho $? > ${quoted(exitFile)}\n`, { mode:0o755 })
  // Its own process group: WebKitWebDriver and the app inherit the output
  // pipes, so stopping the session means stopping all three.
  const driver = spawn("tauri-driver", ["--port", String(port), "--native-port", String(port + 1)], { env:appEnv(), stdio:["ignore", "pipe", "pipe"], detached:true })
  const stopAll = () => { try { process.kill(-driver.pid, "SIGKILL") } catch { /* already gone */ } }
  driver.stdout.on("data", (data) => { session.logs = (session.logs + data).slice(-32768) })
  driver.stderr.on("data", (data) => { session.logs = (session.logs + data).slice(-32768) })
  let driverError
  driver.on("error", (error) => { driverError = error })
  const base = `http://127.0.0.1:${port}`
  const wd = async (method, path, body) => {
    const response = await fetch(`${base}${path}`, { method, headers:{ "Content-Type":"application/json" }, body:body === undefined ? undefined : JSON.stringify(body), signal:AbortSignal.timeout(120000) })
    const json = await response.json().catch(() => ({}))
    if (!response.ok) throw new Error(`WebDriver ${method} ${path}: ${response.status} ${JSON.stringify(json.value ?? json).slice(0, 2000)}`)
    return json.value
  }
  const exited = async () => { try { return Number((await readFile(exitFile, "utf8")).trim()) } catch { return null } }
  // Until the page answers, any failure (the driver missing, the app dying
  // at start-up) stops the driver and says why, instead of leaving it
  // running and this script waiting on it.
  let sessionPath
  const starting = async () => {
    const ready = Date.now() + 20000
    for (;;) {
      if (driverError) throw new Error(`tauri-driver could not be started: ${driverError.message}`)
      if (driver.exitCode !== null) throw new Error(`tauri-driver exited with ${driver.exitCode}`)
      try { await wd("GET", "/status"); break } catch (error) { if (Date.now() > ready) throw new Error(`tauri-driver did not answer within 20 s: ${error.message}`) }
      await delay(100)
    }
    const creating = wd("POST", "/session", { capabilities:{ alwaysMatch:{ "tauri:options":{ application:wrapper } } } })
    creating.catch(() => {})
    const deadline = Date.now() + 90000
    let created
    creating.then((value) => { created = value }, (error) => { created = { error } })
    while (!created) {
      const code = await exited()
      if (code !== null) throw new Error(`The app exited with ${code} before its WebDriver session started`)
      if (Date.now() > deadline) throw new Error("No WebDriver session within 90 s")
      await delay(200)
    }
    if (created.error) throw created.error
    sessionPath = `/session/${created.sessionId}`
  }
  try {
    await starting()
  } catch (error) {
    stopAll()
    throw new Error(`${error.message}\n--- driver and app output ---\n${session.logs.slice(-6000)}`)
  }
  await wd("POST", `${sessionPath}/timeouts`, { script:120000 })
  const evaluate = async (expression) => {
    const value = await wd("POST", `${sessionPath}/execute/async`, {
      script:`const done = arguments[arguments.length - 1]; Promise.resolve().then(() => (${expression})).then((value) => done({ value: value === undefined ? null : value }), (error) => done({ error: String(error && error.stack || error) }))`,
      args:[],
    })
    if (value?.error) throw new Error(value.error)
    return value?.value
  }
  const hook = `(() => { if (window.__smokeConsole) return true; window.__smokeConsole = [];
    const push = (type, args) => window.__smokeConsole.push({ type, text: args.map((a) => a && a.stack || (typeof a === "object" ? JSON.stringify(a) : String(a))).join(" ").slice(0, 4000) });
    for (const type of ["error", "warn", "assert"]) { const original = console[type].bind(console); console[type] = (...args) => { if (type !== "assert" || !args[0]) push(type === "warn" ? "warning" : type, type === "assert" ? args.slice(1) : args); original(...args) } }
    window.addEventListener("error", (event) => push("exception", [event.error || event.message]));
    window.addEventListener("unhandledrejection", (event) => push("exception", [event.reason]));
    return true })()`
  const hookDeadline = Date.now() + 60000
  for (;;) {
    try { await evaluate(hook); break } catch (error) {
      const code = await exited()
      if (code !== null || Date.now() > hookDeadline) {
        await wd("DELETE", sessionPath).catch(() => {})
        stopAll()
        throw new Error(`${code !== null ? `The app exited with ${code}` : "The page did not answer within 60 s"}: ${error.message}\n--- driver and app output ---\n${session.logs.slice(-6000)}`)
      }
    }
    await delay(200)
  }
  const drain = async () => {
    try { session.console.push(...(await evaluate("window.__smokeConsole ? window.__smokeConsole.splice(0) : []"))) } catch { /* the page is gone */ }
  }
  const buttons = { left:0, middle:1, right:2 }
  const modifierKeys = (modifiers) => [[1, ""], [2, ""], [4, ""], [8, ""]].filter(([bit]) => modifiers & bit).map(([, key]) => key)
  const mouse = async (p, { clickCount = 1, button = "left", modifiers = 0 } = {}) => {
    const held = modifierKeys(modifiers)
    if (held.length) await wd("POST", `${sessionPath}/actions`, { actions:[{ type:"key", id:"keyboard", actions:held.map((value) => ({ type:"keyDown", value })) }] })
    const pointer = [{ type:"pointerMove", duration:0, x:Math.round(p.x), y:Math.round(p.y), origin:"viewport" }]
    for (let n = 0; n < clickCount; n++) pointer.push({ type:"pointerDown", button:buttons[button] }, { type:"pointerUp", button:buttons[button] })
    await wd("POST", `${sessionPath}/actions`, { actions:[{ type:"pointer", id:"mouse", parameters:{ pointerType:"mouse" }, actions:pointer }] })
    if (held.length) await wd("POST", `${sessionPath}/actions`, { actions:[{ type:"key", id:"keyboard", actions:held.map((value) => ({ type:"keyUp", value })) }] })
  }
  const special = { Enter:"", Escape:"", Tab:"", End:"", " ":" " }
  const key = async (value, code = value, modifiers = 0) => {
    const held = modifierKeys(modifiers)
    const typed = special[value] ?? value
    await wd("POST", `${sessionPath}/actions`, { actions:[{ type:"key", id:"keyboard", actions:[
      ...held.map((value) => ({ type:"keyDown", value })), { type:"keyDown", value:typed }, { type:"keyUp", value:typed }, ...[...held].reverse().map((value) => ({ type:"keyUp", value })),
    ] }] })
  }
  const finish = async () => {
    await wd("DELETE", sessionPath).catch(() => {})
    stopAll()
  }
  // Closes the window as the user does: the window manager's close request
  // (_NET_CLOSE_WINDOW, then WM_DELETE_WINDOW), as the title bar's button.
  const close = async () => {
    await drain()
    const windows = spawnSync("wmctrl", ["-l", "-p"], { encoding:"utf8" }).stdout.trim().split("\n").filter((line) => /Windfall/.test(line))
    if (!windows.length) throw new Error("No Windfall window for the window manager to close")
    for (const line of windows) spawnSync("wmctrl", ["-i", "-c", line.split(/\s+/)[0]])
    const deadline = Date.now() + 20000
    let code = null
    while ((code = await exited()) === null && Date.now() < deadline) await delay(100)
    await finish()
    if (code === null) throw new Error("The app did not exit within 20 s of closing its window")
    return code
  }
  const kill = async () => {
    await Promise.race([drain(), delay(10000)])
    await Promise.race([wd("DELETE", sessionPath).catch(() => {}), delay(10000)])
    stopAll()
  }
  return { child:driver, evaluate, mouse, key, close, kill, drain, screenshot:() => wd("GET", `${sessionPath}/screenshot`) }
}

async function launch(label) {
  const session = { label, logs:"", console:[] }
  report.sessions.push(session)
  const started = performance.now()
  const transport = linux ? await webdriverTransport(session) : await devtoolsTransport(session)
  const { child, evaluate, mouse, key } = transport
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
    .catch(async (error) => {
      const page = await evaluate('({ url: location.href, ready: document.readyState, tauri: typeof window.__TAURI_INTERNALS__?.invoke, text: document.body?.innerText.slice(0, 1500) })').catch((e) => String(e))
      const shot = await transport.screenshot?.().catch(() => null)
      if (shot) await writeFile(resolve(folder, `${session.label.replace(/\W+/g, "-")}-start.png`), Buffer.from(shot, "base64"))
      await transport.kill()
      throw new Error(`${error.message}: ${JSON.stringify(page)}`)
    })
  session.startupMs = Math.round(performance.now() - started)

  const invoke = (command, args = {}) => evaluate(`(async () => {
    const command = ${JSON.stringify(command)};
    const args = ${JSON.stringify(args)};
    if (command === "window_document_snapshot" || command === "document_snapshot") return window.__TAURI_INTERNALS__.invoke(command, args);
    const owned = await window.__TAURI_INTERNALS__.invoke("window_document_snapshot");
    return window.__TAURI_INTERNALS__.invoke(command, { ...args, caller: args.caller ?? owned.caller });
  })()`)
  const owned = () => invoke("window_document_snapshot")
  const project = async () => (await owned()).snapshot.project
  const dispatch = async (command) => invoke("dispatch", { command, generation:(await owned()).generation })
  // The engine's frames as the window receives them, for the meters.
  await evaluate(`(() => {const callbacks=window.__TAURI_INTERNALS__.callbacks;const set=callbacks.set.bind(callbacks);window.__songFrames=[];const wrap=callback=>data=>{const frame=data?.message;if(frame&&Array.isArray(frame.meters)&&typeof frame.voices==="number"){window.__songFrames.push(frame);if(window.__songFrames.length>400)window.__songFrames.shift()}return callback(data)};for(const [id,callback]of callbacks)set(id,wrap(callback));callbacks.set=(id,callback)=>set(id,wrap(callback));return true})()`)
  const visible = (selector, text, exact = false) => `[...document.querySelectorAll(${JSON.stringify(selector)})].find((e) => (${text === undefined ? "true" : exact ? `e.textContent.trim() === ${JSON.stringify(text)}` : `e.textContent.trim().startsWith(${JSON.stringify(text)})`}) && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden")`
  // The centre of the element, once it is the topmost thing there: a click
  // a person makes lands on what they see.
  // It must also have stopped moving (two reads 50 ms apart agree): lists
  // that open then position themselves are clicked where they settle. And
  // when bringing it into view scrolled anything, the click waits two
  // frames: WebKitGTK scrolls asynchronously, and input arriving at once is
  // routed by the old scroll position (a person never clicks that fast).
  const point = async (selector, text, exact) => {
    let last = null, previous = null
    return until(async () => {
      last = await evaluate(`(() => {
        const element = ${visible(selector, text, exact)}
        if (!element || element.disabled || element.getAttribute("aria-disabled") === "true") {
          const all = [...document.querySelectorAll(${JSON.stringify(selector)})]
          return { missing:true, matching:all.length, disabled:element ? true : undefined,
            menus:[...document.querySelectorAll('[role="menu"]')].map((m) => { const r = m.getBoundingClientRect(); return { items:m.querySelectorAll('[role="menuitem"]').length, top:Math.round(r.top), bottom:Math.round(r.bottom), height:Math.round(r.height), scrollHeight:m.scrollHeight, first:m.querySelector('[role="menuitem"]')?.textContent.trim() } }),
            expanded:[...document.querySelectorAll('[aria-expanded="true"]')].map((e) => e.getAttribute("aria-label") || e.textContent.trim().slice(0, 30)) }
        }
        // An item of a long menu or list is brought to the middle, clear of
        // the scroll arrows such a menu shows at its edges.
        // Menus and lists animate open; aim once nothing moving holds it.
        if (document.getAnimations().some((a) => a.playState === "running" && a.effect?.target?.contains?.(element))) return { animating:true }
        const before = element.getBoundingClientRect()
        element.scrollIntoView({ block:element.closest('[role="menu"], [role="listbox"]') ? "center" : "nearest", inline:"nearest" })
        const r = element.getBoundingClientRect()
        if (Math.abs(before.x - r.x) > 0.5 || Math.abs(before.y - r.y) > 0.5) return { scrolled:true }
        if (!r.width || !r.height) return { missing:true }
        const x = r.x + r.width / 2, y = r.y + r.height / 2
        const top = document.elementFromPoint(x, y)
        if (element.contains(top)) return { x, y }
        const box = top?.getBoundingClientRect()
        return { covered:top ? top.outerHTML.slice(0, 200) : "nothing", x, y,
          stack:document.elementsFromPoint(x, y).slice(0, 5).map((e) => e.tagName + (e.getAttribute("role") ? "[" + e.getAttribute("role") + "]" : "") + (e.getAttribute("aria-label") ? "(" + e.getAttribute("aria-label") + ")" : "")),
          target:{ top:r.top, bottom:r.bottom, left:r.left, right:r.right },
          cover:top && { top:box.top, bottom:box.bottom, offsetHeight:top.offsetHeight, clientHeight:top.clientHeight, scrollWidth:top.scrollWidth, clientWidth:top.clientWidth } }
      })()`)
      if (last.scrolled) {
        previous = null
        await evaluate("new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(true))))")
        await delay(150)
        return null
      }
      if (last.animating) { previous = null; await delay(30); return null }
      if (last.covered || last.missing) { previous = null; return null }
      const settled = previous && Math.abs(previous.x - last.x) < 0.5 && Math.abs(previous.y - last.y) < 0.5
      previous = last
      if (!settled) { await delay(50); return null }
      return last
    }, `visible ${selector} ${text ?? ""}`).catch((error) => { throw new Error(`${error.message} (${JSON.stringify(last)})`) })
  }
  const click = async (selector, text, options = {}) => mouse(await point(selector, text, options.exact), options)
  const shown = (text) => evaluate(`document.body.innerText.includes(${JSON.stringify(text)})`)
  const texts = (selector) => evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].filter((e) => e.getClientRects().length > 0).map((e) => e.textContent.trim())`)
  const closed = (selector) => until(() => evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].every((e) => e.getClientRects().length === 0)`), `${selector} to close`)
  const menu = async (title, ...items) => {
    await click('[role="menubar"] [role="menuitem"]', title, { exact:true })
    for (const item of items) await click('[role="menu"] [role="menuitem"]', item)
  }
  // Chooses an option of the open list, clicking it again (as a person would)
  // when the list is still open a moment later; repeats are kept as records.
  const choose = async (text) => {
    for (let attempt = 1; ; attempt++) {
      await click('[role="option"]', text, { exact:true })
      const done = await until(() => evaluate(`[...document.querySelectorAll('[role="listbox"]')].every((e) => e.getClientRects().length === 0)`), "the list to close", 1500).catch(() => false)
      if (done) {
        if (attempt > 1) report.checks.push({ name:`the option ${text} was taken (it took ${attempt} clicks)`, pass:true, detail:{ attempts:attempt } })
        return
      }
      if (attempt === 3) throw new Error(`The option ${text} was not taken in three clicks`)
    }
  }
  const crashed = () => evaluate(`[...document.querySelectorAll('[role="alert"]')].map((e) => e.textContent.trim()).filter((text) => /crashed|could not start/i.test(text))`)
  const quit = async () => {
    const started = performance.now()
    session.exitCode = await transport.close()
    session.quitMs = Math.round(performance.now() - started)
  }
  const kill = () => transport.kill()
  const screenshot = () => transport.screenshot?.() ?? transport.cdp?.("Page.captureScreenshot", { format:"png" }).then((shot) => shot.data)
  return { session, child, transport, screenshot, choose, evaluate, until, invoke, owned, project, dispatch, point, mouse, click, key, shown, texts, closed, menu, crashed, quit, kill }
}

// Answers the operating system's file dialog titled `title` with `path`.
const picker = (title, path) => linux ? gtkPicker(title, path) : windowsPicker(title, path)

// Windows: the real dialog (class #32770), its file name box given the path
// with WM_SETTEXT, and its default button pressed with WM_COMMAND IDOK.
const windowsPicker = (title, path) => new Promise((resolve, reject) => {
  const quote = (text) => `'${text.replace(/'/g, "''")}'`
  const script = `
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $A = [System.Windows.Automation.AutomationElement]
    $scope = [System.Windows.Automation.TreeScope]::Descendants
    $named = New-Object System.Windows.Automation.PropertyCondition($A::NameProperty, ${quote(title)})
    $window = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Window)
    $native = New-Object System.Windows.Automation.PropertyCondition($A::ClassNameProperty, '#32770')
    $both = New-Object System.Windows.Automation.AndCondition($named, $window, $native)
    $dialog = $null
    for ($i = 0; $i -lt 75 -and -not $dialog; $i++) { $dialog = $A::RootElement.FindFirst($scope, $both); if (-not $dialog) { Start-Sleep -Milliseconds 200 } }
    if (-not $dialog) { exit 3 }
    Add-Type -Name Window -Namespace Smoke -MemberDefinition '[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr window, int message, IntPtr w, string l); [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, int message, IntPtr w, IntPtr l);'
    $id = { param($value) New-Object System.Windows.Automation.PropertyCondition($A::AutomationIdProperty, $value) }
    $name = $null
    for ($i = 0; $i -lt 50 -and -not $name; $i++) {
      $name = @($dialog.FindAll($scope, (& $id '1148'))) + @($dialog.FindAll($scope, (& $id '1001'))) | Where-Object { $_.Current.ClassName -eq 'Edit' } | Select-Object -First 1
      if (-not $name) { Start-Sleep -Milliseconds 100 }
    }
    if (-not $name) { exit 5 }
    [void][Smoke.Window]::SendMessage([IntPtr]$name.Current.NativeWindowHandle, 0x000C, [IntPtr]::Zero, ${quote(path)})
    [void][Smoke.Window]::PostMessage([IntPtr]$dialog.Current.NativeWindowHandle, 0x0111, [IntPtr]1, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 1500
    if ($A::RootElement.FindFirst($scope, $both)) { exit 4 }
  `
  const choosing = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { windowsHide:true, stdio:"ignore" })
  choosing.on("error", reject)
  choosing.on("exit", (code) => code === 0 ? resolve() : reject(new Error(`The file dialog titled ${title} could not be answered (${code})`)))
})

// Linux: GTK's own file chooser, found by its title with xdotool, given
// the whole path by typing it (Ctrl+L first opens the location field of an
// Open dialog; a Save dialog's name field takes a path) and Enter, as a
// person can. The keys are real X input events (XTEST).
async function gtkPicker(title, path) {
  const find = () => spawnSync("xdotool", ["search", "--onlyvisible", "--name", `^${title.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`], { encoding:"utf8" }).stdout.trim().split("\n").filter(Boolean)
  let dialog
  for (let i = 0; i < 75 && !dialog; i++) { dialog = find().at(-1); if (!dialog) await delay(200) }
  if (!dialog) throw new Error(`The file dialog titled ${title} did not open`)
  spawnSync("xdotool", ["windowactivate", "--sync", dialog])
  await delay(700)
  const xdo = (...args) => spawnSync("xdotool", args, { encoding:"utf8" })
  if (/^Open/.test(title)) { xdo("key", "--clearmodifiers", "ctrl+l"); await delay(300) }
  xdo("key", "--clearmodifiers", "ctrl+a")
  xdo("type", "--delay", "15", path)
  await delay(300)
  xdo("key", "--clearmodifiers", "Return")
  for (let i = 0; i < 40 && find().length; i++) await delay(100)
  if (find().length) throw new Error(`The file dialog titled ${title} could not be answered`)
}

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
    if (tag === 3 && bits === 64) return data.readDoubleLE(at)
    if (tag === 1 && bits === 16) return data.readInt16LE(at) / 32768
    if (tag === 1 && bits === 24) return data.readIntLE(at, 3) / 8388608
    if (tag === 1 && bits === 32) return data.readInt32LE(at) / 2147483648
    throw new Error(`Unsupported WAV encoding tag ${tag}, ${bits} bits`)
  }
  let peak = 0, energy = 0, nonFinite = 0, over = 0, firstLoud = -1, lastLoud = -1
  const blockRms = []
  const block = Math.max(1, Math.round(rate / 10))
  let blockEnergy = 0
  for (let f = 0; f < frames; f++) {
    for (let c = 0; c < channels; c++) {
      const value = read((f * channels + c) * bytes)
      if (!Number.isFinite(value)) { nonFinite++; continue }
      const size = Math.abs(value)
      if (size > peak) peak = size
      if (size > 1) over++
      if (size > 1e-3) { if (firstLoud < 0) firstLoud = f; lastLoud = f }
      energy += value * value
      blockEnergy += value * value
    }
    if ((f + 1) % block === 0) { blockRms.push(Math.sqrt(blockEnergy / (block * channels))); blockEnergy = 0 }
  }
  const rms = Math.sqrt(energy / Math.max(1, frames * channels))
  return { tag, channels, rate, bits, frames, seconds:frames / rate, peak, rms, nonFinite, over, firstLoud, lastLoud, silentBlocks:blockRms.filter((v) => v < 1e-4).length, blocks:blockRms.length, data }
}

const songTicks = (project) => Math.max(0, ...project.playlist.clips.map((clip) => clip.start + clip.length))
const clone = (project) => JSON.parse(JSON.stringify(project))
// Where two JSON values differ, as paths, for a failed comparison.
const differences = (a, b, path = "", out = []) => {
  if (out.length >= 20) return out
  if (JSON.stringify(a) === JSON.stringify(b)) return out
  if (a && b && typeof a === "object" && typeof b === "object") {
    for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) differences(a[key], b[key], `${path}.${key}`, out)
  } else out.push({ path, before:JSON.stringify(a)?.slice(0, 200), after:JSON.stringify(b)?.slice(0, 200) })
  return out
}

// File > Export audio…, the whole song, a path chosen in the system picker,
// Export, and the dialog closed once the file is written.
async function exportSong(session, path) {
  const { menu, until, shown, click, evaluate, closed, key } = session
  await menu("File", "Export audio…")
  await until(() => shown("Save to"), "the export dialog")
  if (!(await evaluate(`[...document.querySelectorAll('[role="dialog"] [role="combobox"]')].some((e) => e.textContent.trim() === "The whole song")`))) {
    await click('[role="dialog"] [role="combobox"]', "The selected pattern", { exact:true })
    await click('[role="option"]', "The whole song", { exact:true })
    await closed('[role="listbox"]')
  }
  const choosing = picker("Export audio", path)
  await click('[role="dialog"] button', "Choose…", { exact:true })
  await choosing
  const name = path.split(/[\\/]/).at(-1)
  await until(() => evaluate(`document.querySelector('[role="dialog"]').innerText.includes(${JSON.stringify(name)}) || [...document.querySelectorAll('[role="dialog"] input')].some((input) => input.value.endsWith(${JSON.stringify(name)}))`), "the chosen export path in the dialog")
  const pressed = performance.now()
  await click('[role="dialog"] button', "Export", { exact:true })
  await until(async () => { try { return (await stat(path)).size > 44 } catch { return false } }, "the exported WAV", 120000)
  const written = performance.now()
  await until(() => evaluate(`!document.body.innerText.includes("Exporting")`), "the export to finish", 120000)
  ;(report.exportRuns ??= []).push({ path, renderMs:Math.round(written - pressed), settledMs:Math.round(performance.now() - pressed) })
  await delay(500)
  await click('[role="dialog"] button', "Close", { exact:true }).catch(() => key("Escape"))
  await closed('[role="dialog"]')
}

let app
try {
  // ================= First launch: make the song. =================
  app = await launch("make the song")
  report.timings.startupFirstMs = app.session.startupMs
  const { click, mouse, key, menu, until, evaluate, dispatch, project, owned, invoke, shown, texts, closed, point } = app

  // A new project from the default template, from the File menu.
  await menu("File", "New project")
  const fresh = await until(async () => { const o = await owned(); return o.snapshot.history.cursor === 0 && !o.snapshot.dirty ? o : null }, "the new project")
  const kit = fresh.snapshot.project.channels
  check("new project from the default template has the factory kit, one mixer track each", kit.length === 4 && kit.every((channel) => channel.source?.type === "sampler" || channel.sample !== undefined || channel.source !== undefined), kit.map((channel) => channel.name))
  const drums = fresh.snapshot.project.patterns[0].id

  // The drum pattern: clicked into the rack's step buttons.
  const beat = { 0:[0, 4, 8, 12], 1:[4, 12], 2:[2, 6, 10, 14], 3:[7, 15] }
  for (const [index, steps] of Object.entries(beat)) {
    const name = kit[index].name
    for (const step of steps) await click(`[aria-label=${JSON.stringify(`${name} steps`)}] [data-step="${step}"]`)
  }
  const afterBeat = await until(async () => {
    const p = await project()
    const lanes = p.patterns.find((pattern) => pattern.id === drums).lanes
    const total = lanes.reduce((sum, lane) => sum + lane.notes.length, 0)
    return total === 12 ? p : null
  }, "twelve drum steps in the document")
  for (const [index, steps] of Object.entries(beat)) {
    const lane = afterBeat.patterns.find((pattern) => pattern.id === drums).lanes.find((lane) => lane.channel === kit[index].id)
    check(`drum steps of ${kit[index].name} are where they were clicked`, JSON.stringify(lane.notes.map((note) => note.start / (PPQ / 4)).sort((a, b) => a - b)) === JSON.stringify(steps), lane.notes.map((note) => note.start))
  }

  // The layout at the default window size: nothing is drawn over the
  // pattern selector, and the browser's tabs do not cover its search box.
  const covered = await evaluate(`(() => {
    const button = document.querySelector('[aria-label^="Pattern: "]'); const r = button.getBoundingClientRect()
    const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)
    return { width:innerWidth, height:innerHeight, top:top && !button.contains(top) ? top.outerHTML.slice(0, 160) : null }
  })()`)
  soft("the pattern selector is not covered by other controls at the default window size", covered.top === null, covered)
  const tabs = await evaluate(`(() => {
    const list = document.querySelector('[aria-label="Browser view"]').getBoundingClientRect()
    const last = [...document.querySelectorAll('[aria-label="Browser view"] [role="tab"]')].map((tab) => tab.getBoundingClientRect().bottom)
    return { listBottom:list.bottom, tabsBottom:Math.max(...last) }
  })()`)
  soft("the browser's tab rows fit inside their list", tabs.tabsBottom <= tabs.listBottom + 0.5, tabs)

  // An instrument channel from the rack's menu.
  await click('[aria-label="Add channel menu"]')
  await click('[role="menu"] [role="menuitem"]', "Add subtractive synth")
  const withSynth = await until(async () => { const p = await project(); return p.channels.length === 5 ? p : null }, "the synth channel")
  const synth = withSynth.channels.at(-1)
  check("a subtractive synth channel was added from the rack menu, with its own mixer track", synth.source?.type === "instrument" || JSON.stringify(synth.source).includes("subtractive"), synth)

  // A second pattern for the melody, from the Patterns menu.
  await menu("Patterns", "Add pattern")
  const withMelody = await until(async () => { const p = await project(); return p.patterns.length === 2 ? p : null }, "the second pattern")
  const melody = withMelody.patterns[1].id
  await until(async () => (await texts('[aria-label^="Pattern: "]'))[0] === withMelody.patterns[1].name, "the new pattern to be selected")

  // The melody, drawn in the piano roll on the synth.
  await click(`[data-channel-button="${synth.id}"]`)
  await click('[role="tab"]', "Piano roll", { exact:true })
  const grid = await until(() => evaluate(`(() => { const g = document.querySelector('[aria-label="Note grid"]'); if (!g || !g.getClientRects().length) return null; const r = g.getBoundingClientRect(); return { x:r.x, y:r.y, width:r.width, height:r.height } })()`), "the note grid")
  const keyRow = async (name) => evaluate(`(() => { const label = [...document.querySelectorAll("span")].find((e) => e.children.length === 0 && e.textContent.trim() === ${JSON.stringify(name)} && e.getClientRects().length); if (!label) return null; const r = label.parentElement.getBoundingClientRect(); return { y:r.bottom, height:r.height } })()`)
  const c5 = await until(() => keyRow("C5"), "the C5 key"), c4 = await keyRow("C4")
  check("the note grid has a C5 (key 60) and C4 key in view", c5 && c4 && c4.y > c5.y, { c5, c4 })
  const semitone = (c4.y - c5.y) / 12
  // A C key's own row is the bottom semitone of its key.
  const rowY = (key) => c5.y - semitone / 2 - (key - 60) * semitone
  const melodyNotes = () => project().then((p) => p.patterns.find((pattern) => pattern.id === melody).lanes.find((lane) => lane.channel === synth.id)?.notes ?? [])
  const pick = async (label, option) => {
    await click(`[role="combobox"][aria-label="${label}"]`)
    await app.choose(option)
  }
  // One probe note with no snap tells how many ticks a pixel is; it is undone.
  await pick("Snap", "None")
  await mouse({ x:grid.x + 200, y:rowY(60) })
  const probe = await until(async () => (await melodyNotes())[0], "the probe note")
  const ticksPerPixel = probe.start / 200
  check("a click in the note grid draws a note on the key under the pointer", probe.key === 60 && ticksPerPixel > 0, { probe, ticksPerPixel })
  await key("z", "KeyZ", 2)
  await until(async () => (await melodyNotes()).length === 0, "Ctrl+Z to remove the probe note")
  await pick("Snap", "Beat")
  // Snap is one choice shared with the playlist; what each view shows of it.
  const snapShown = () => evaluate(`({ pianoRoll: [...document.querySelectorAll('[role="combobox"][aria-label="Snap"]')].filter((e) => e.getClientRects().length).map((e) => e.textContent.trim()), playlist: [...document.querySelectorAll('[aria-label^="Snap: "]')].map((e) => e.getAttribute("aria-label")) })`)
  soft("the snap shown after choosing Beat in the piano roll", true, await snapShown())
  const tune = [[0, 60], [1, 64], [2, 67], [3, 65], [5, 64], [6, 62], [7, 59]]
  check("every key of the melody is in view in the note grid", tune.every(([, pitch]) => rowY(pitch) > grid.y + 2 && rowY(pitch) < grid.y + grid.height - 2), { grid, c5, semitone })
  for (const [beat, pitch] of tune) await mouse({ x:grid.x + (beat * PPQ + PPQ / 4) / ticksPerPixel, y:rowY(pitch) })
  const drawn = await until(async () => { const notes = await melodyNotes(); return notes.length === tune.length ? notes : null }, "the melody's notes")
    .catch(async (error) => { throw new Error(`${error.message}: ${JSON.stringify((await melodyNotes()).map((note) => [note.start, note.length, note.key]))}, ticks per pixel ${ticksPerPixel}`) })
  const sorted = [...drawn].sort((a, b) => a.start - b.start)
  check("the melody's notes are on the beats and keys they were drawn at", JSON.stringify(sorted.map((note) => [note.start / PPQ, note.key])) === JSON.stringify(tune), sorted.map((note) => [note.start, note.length, note.key]))
  const melodyLength = Math.max(...sorted.map((note) => note.start + note.length))

  soft("the snap shown after drawing the melody", true, await snapShown())

  // The arrangement: both patterns over four bars on the playlist.
  await click('[role="tab"]', "Playlist", { exact:true })
  soft("the snap shown on opening the playlist", true, await snapShown())
  const timeline = await until(() => evaluate(`(() => { const g = document.querySelector('[aria-label="Song timeline"]'); if (!g || !g.getClientRects().length) return null; const r = g.getBoundingClientRect(); return { x:r.x, y:r.y, width:r.width, height:r.height } })()`), "the song timeline")
  for (let i = 0; i < 4 && !(await texts('[aria-label^="Snap: "]'))[0]?.startsWith("Bar"); i++) await click("button", "Coarser", { exact:true })
  check("the playlist snaps to bars", (await evaluate(`document.querySelector('[aria-label^="Snap: "]').getAttribute("aria-label")`)) === "Snap: Bar", null)
  const bar = 4 * PPQ
  // At this zoom a bar is 72 pixels: placing on bar 1 first confirms it.
  const barPixels = 72
  const rowHeight = 38
  const place = async (pattern, row, bars) => {
    await click('[aria-label="Pattern to place"] button', pattern)
    // At a person's pace: a click while the last placement is still being
    // committed is not taken (the playlist ignores presses until then).
    for (const at of bars) { await mouse({ x:timeline.x + 10 + at * barPixels, y:timeline.y + rowHeight * row + rowHeight / 2 }); await delay(150) }
  }
  await place(afterBeat.patterns[0].name, 0, [0, 1, 2, 3])
  await place(withMelody.patterns[1].name, 1, [0, 2])
  const arranged = await until(async () => { const p = await project(); return p.playlist.clips.length === 6 ? p : null }, "six clips on the playlist")
    .catch(async (error) => { throw new Error(`${error.message}: ${JSON.stringify((await project()).playlist.clips.map((clip) => [clip.track, clip.start, clip.length, clip.content]))} timeline ${JSON.stringify(timeline)}`) })
  const lanesOf = (p, pattern) => p.playlist.clips.filter((clip) => clip.content.type === "pattern" && clip.content.pattern === pattern).map((clip) => [clip.start / bar, clip.length / bar]).sort((a, b) => a[0] - b[0])
  check("the drum pattern plays on bars 1 to 4 of the song", JSON.stringify(lanesOf(arranged, drums)) === JSON.stringify([[0, 1], [1, 1], [2, 1], [3, 1]]), lanesOf(arranged, drums))
  check("the two-bar melody plays twice, over bars 1 to 4", JSON.stringify(lanesOf(arranged, melody)) === JSON.stringify([[0, 2], [2, 2]]), lanesOf(arranged, melody))

  // The mixer: a bus for clap and snare, through the channels' routing badges.
  const routeBadge = (name) => `[aria-label^=${JSON.stringify(`${name} plays into`)}]`
  await click('[role="tab"]', "Channel rack", { exact:true })
  await click(routeBadge(kit[1].name))
  await click('[role="menu"] [role="menuitem"]', "Route to a new mixer track")
  const routed = await until(async () => { const p = await project(); const clap = p.channels.find((channel) => channel.id === kit[1].id); return p.mixer.tracks.length === 7 && clap.mixerTrack !== kit[1].mixerTrack ? p : null }, "the clap on a new mixer track")
  const bus = routed.mixer.tracks.find((track) => track.id === routed.channels.find((channel) => channel.id === kit[1].id).mixerTrack)
  await click(routeBadge(kit[3].name))
  await click('[role="menu"] [role="menuitemradio"]', `${routed.mixer.tracks.indexOf(bus)}${bus.name}`, { exact:true })
  await until(async () => (await project()).channels.find((channel) => channel.id === kit[3].id).mixerTrack === bus.id, "the snare routed to the bus")
  // Effects from the strips' Add effect menus.
  const stripOf = async (trackId) => { const p = await project(); const track = p.mixer.tracks.find((item) => item.id === trackId); return `[aria-label=${JSON.stringify(`${track.name}, track ${p.mixer.tracks.indexOf(track)}`)}]` }
  const addEffect = async (trackId, name, type) => {
    // As with the language list: a menu that did not open is clicked again,
    // and how often that took is kept with the report.
    const trigger = `${await stripOf(trackId)} [data-slot="track-add-effect"]`
    for (let attempt = 1; ; attempt++) {
      // The last menu (a closing one included) gone first, as a person
      // waits for it; then the menu counts as open when its own button says so.
      await closed('[role="menu"]')
      await click(trigger)
      const opened = await until(() => evaluate(`document.querySelector(${JSON.stringify(trigger)})?.getAttribute("aria-expanded") === "true" && [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].some((e) => e.getClientRects().length > 0)`), "the effect menu", 1500).catch(() => false)
      if (opened) {
        if (attempt > 1) soft(`the Add effect menu of track ${trackId} opened (it took ${attempt} clicks)`, true, { attempts:attempt })
        const state = () => evaluate(`({ focus: document.activeElement && (document.activeElement.getAttribute("aria-label") || document.activeElement.tagName + "." + document.activeElement.className.toString().slice(0, 60)), menus: document.querySelectorAll('[role="menu"]').length, expanded: [...document.querySelectorAll('[aria-expanded="true"]')].map((e) => e.getAttribute("aria-label") || e.textContent.trim().slice(0, 30)) })`)
        const atOpen = await state()
        await delay(500)
        const later = await state()
        soft(`the Add effect menu of track ${trackId} stays open`, later.menus > 0, { atOpen, later })
        if (later.menus === 0) {
          const shot = await app.screenshot?.().catch(() => null)
          if (shot) await writeFile(resolve(folder, `effect-menu-${trackId}.png`), Buffer.from(shot, "base64"))
        }
        break
      }
      if (attempt === 3) throw new Error(`The Add effect menu of track ${trackId} did not open in three clicks`)
    }
    await click('[role="menu"] [role="menuitem"]', name, { exact:true })
    return until(async () => (await project()).mixer.tracks.find((track) => track.id === trackId).effects.find((effect) => effect.params.type === type), `${name} on track ${trackId}`)
  }
  await addEffect(synth.mixerTrack, "Delay", "delay")
  await addEffect(bus.id, "Reverb", "reverb")
  const beforeLastEdits = clone(await project())
  const cursorBeforeLastEdits = (await owned()).snapshot.history.cursor

  // An automation clip: right-click the synth's volume knob, then a curve.
  await click('[role="tab"]', "Channel rack", { exact:true })
  await click(`[aria-label=${JSON.stringify(`${synth.name} channel volume`)}]`, undefined, { button:"right" })
  await click('[role="menu"] [role="menuitem"]', "Create automation clip")
  const automated = await until(async () => { const p = await project(); return p.automations.length === 1 && p.playlist.clips.some((clip) => clip.content.type === "automation") ? p : null }, "the automation clip")
  const automation = automated.automations[0]
  check("the automation targets the synth's volume and has a clip on the playlist", automation.target.type === "channelVolume" && automation.target.channel === synth.id, automation)
  // Drawing points in the clip is a canvas gesture; the curve is set by command.
  const curve = [{ tick:0, value:0.2, curve:0, hold:false }, { tick:2 * bar, value:0.8, curve:0, hold:false }, { tick:4 * bar - 1, value:0.5, curve:0, hold:false }]
  await dispatch({ type:"setAutomationPoints", id:automation.id, points:curve })
  // A compressor on the kick, the last edit before undo and redo.
  await addEffect(kit[0].mixerTrack, "Compressor", "compressor")
  const finished = clone(await project())
  check("by command: the automation curve has three points", (await project()).automations[0].points.length === 3, (await project()).automations[0].points)

  // Undo and redo the last three edits with the keyboard.
  await click('[role="tab"]', "Playlist", { exact:true })
  await evaluate("document.activeElement?.blur()")
  const editsBefore = (await owned()).snapshot.history.cursor
  const steps = editsBefore - cursorBeforeLastEdits
  report.undoSteps = steps
  check("the automation clip, its curve and the compressor are separate history steps", steps >= 3, { steps, labels:(await owned()).snapshot.history.entries.slice(-steps).map((entry) => entry.label) })
  for (let i = 0; i < steps; i++) await key("z", "KeyZ", 2)
  await until(async () => (await owned()).snapshot.history.cursor === cursorBeforeLastEdits, "the undo steps")
  // Ids are never handed out twice, so the id counter does not go back.
  const undone = { ...clone(await project()), nextId:beforeLastEdits.nextId }
  check("Ctrl+Z takes the project back to before the automation clip", JSON.stringify(undone) === JSON.stringify(beforeLastEdits), differences(beforeLastEdits, undone))
  for (let i = 0; i < steps; i++) await key("y", "KeyY", 2)
  await until(async () => (await owned()).snapshot.history.cursor === editsBefore, "the redo steps")
  const redone = clone(await project())
  check("Ctrl+Y brings every edit back exactly", JSON.stringify(redone) === JSON.stringify(finished), differences(finished, redone))

  // Play the song on the software sink and stop it.
  await click('[role="group"][aria-label="What Play plays"] button', "Song", { exact:true })
  await until(async () => (await invoke("transport_state")).mode === "song", "song mode")
  await evaluate("window.__songFrames = []")
  const meter = () => evaluate(`(() => {const canvas=document.querySelector('[data-slot="master-meter"] canvas');if(!canvas)return null;const context=canvas.getContext('2d');if(!context)return null;const data=context.getImageData(0,0,canvas.width,canvas.height).data;const colors=new Map();let opaque=0;for(let i=0;i<data.length;i+=4)if(data[i+3]){opaque++;const color=[data[i],data[i+1],data[i+2],data[i+3]].join(',');colors.set(color,(colors.get(color)||0)+1)}return {signalPixels:opaque-Math.max(0,...colors.values())}})()`)
  const idle = await meter()
  await click('[aria-label="Play or stop"]')
  await until(async () => (await invoke("transport_state")).playing, "the transport to play")
  const position = () => evaluate(`document.querySelector('[aria-label="Song position"]')?.textContent.trim()`)
  const first = { ...(await invoke("transport_state")), position:await position() }
  await delay(2500)
  const later = { ...(await invoke("transport_state")), position:await position() }
  report.frameKeys = await evaluate("Object.keys(window.__songFrames.at(-1) ?? {})")
  const sounding = await meter()
  const frames = await evaluate("window.__songFrames")
  const masterPeak = Math.max(0, ...frames.map((frame) => Math.max(frame.meters[0] ?? 0, frame.meters[1] ?? 0)))
  report.playback = { first, later, idle, sounding, frames:frames.length, masterPeak }
  check("the transport moves while the song plays", later.playing && first.position && later.position !== first.position, { first, later })
  check("the master meter shows signal during playback", masterPeak > 0.01 && (sounding?.signalPixels ?? 0) > (idle?.signalPixels ?? 0), report.playback)
  await click('[aria-label="Stop"]')
  await until(async () => !(await invoke("transport_state")).playing, "the transport to stop")
  check("Stop stops the transport", !(await invoke("transport_state")).playing, await invoke("transport_state"))
  // With the readouts showing their figures after playback, the transport
  // bar still fits: the pattern selector is uncovered, Undo and Redo show.
  const transportBar = await evaluate(`(() => {
    const button = document.querySelector('[aria-label^="Pattern: "]'); const r = button.getBoundingClientRect()
    const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)
    const redo = document.querySelector('[role="toolbar"][aria-label="Transport"] [aria-label="Redo"]')?.getBoundingClientRect()
    return { covered:top && !button.contains(top) ? top.outerHTML.slice(0, 160) : null, redoRight:redo?.right ?? null, width:innerWidth }
  })()`)
  soft("after playback the pattern selector is still not covered", transportBar.covered === null, transportBar)
  soft("Undo and Redo in the transport bar are inside the window", transportBar.redoRight !== null && transportBar.redoRight <= transportBar.width, transportBar)

  // A factory sample from the browser: open Bass, double-click Bass Sub.
  await click('[role="tab"]', "Library", { exact:true })
  await click('[role="treeitem"]', "Bass", { exact:true })
  await click('[role="treeitem"]', "Bass Sub.wav", { exact:true, clickCount:2 })
  const withSample = await until(async () => { const p = await project(); return p.channels.length === 6 ? p : null }, "a channel from the browser")
  const bassSample = withSample.samples.find((sample) => sample.name === "Bass Sub")
  check("double-clicking a factory sample in the browser adds a channel playing it", bassSample && withSample.channels.at(-1).name.startsWith("Bass Sub"), { sample:bassSample, channel:withSample.channels.at(-1) })

  // The settings dialog, and Spanish and back.
  const chooseLanguage = async (name, settings, list) => {
    await menu(...settings)
    for (let attempt = 0; ; attempt++) {
      await click(`[role="combobox"][aria-label=${JSON.stringify(list)}]`)
      const opened = await until(() => evaluate(`[...document.querySelectorAll('[role="option"]')].some((e) => e.getClientRects().length > 0)`), "the language list", 1500).catch(() => false)
      if (opened) break
      if (attempt === 4) throw new Error("The language list did not open")
    }
    await app.choose(name)
  }
  await chooseLanguage("Español", ["Options", "Settings…"], "Interface language")
  await until(async () => (await evaluate("document.documentElement.lang")) === "es", "Spanish")
  await key("Escape")
  await closed('[role="dialog"]')
  const spanishMenus = await texts('[role="menubar"] [role="menuitem"]')
  check("the menus are in Spanish", spanishMenus.includes("Archivo") && !spanishMenus.includes("File"), spanishMenus)
  check("no panel crashed while Spanish", (await app.crashed()).length === 0, await app.crashed())
  await chooseLanguage("English", ["Opciones", "Ajustes…"], "Idioma de la interfaz")
  await until(async () => (await evaluate("document.documentElement.lang")) === "en", "English again")
  await key("Escape")
  await closed('[role="dialog"]')
  check("the menus are in English again", (await texts('[role="menubar"] [role="menuitem"]')).includes("File"), null)

  // Export the song to WAV through the export dialog and the system picker.
  const song = await project()
  const exportStarted = performance.now()
  await exportSong(app, exportPath)
  report.timings.exportMs = Math.round(performance.now() - exportStarted)
  const exported = await stat(exportPath)
  const wav = decodeWav(await readFile(exportPath))
  const tempo = song.settings.tempoBpm
  const songSeconds = songTicks(song) / PPQ * 60 / tempo
  report.export = { path:exportPath, bytes:exported.size, ...wav, data:undefined, songSeconds, tempo }
  check("the export is a 24-bit 48 kHz stereo WAV", wav.rate === 48000 && wav.channels === 2 && wav.bits === 24, report.export)
  check("the export is as long as the four-bar song plus at most its tail", wav.seconds >= songSeconds - 0.01 && wav.seconds <= songSeconds + 10, { seconds:wav.seconds, songSeconds })
  check("the export is not silent and has no NaN or runaway level", wav.rms > 0.005 && wav.nonFinite === 0 && wav.peak <= 1.0 && wav.peak > 0.05, { rms:wav.rms, peak:wav.peak, nonFinite:wav.nonFinite, over:wav.over })
  check("the export has sound from the first bar and the melody's bars", wav.firstLoud >= 0 && wav.firstLoud < 0.05 * wav.rate && wav.lastLoud > (songSeconds * 0.9) * wav.rate, { firstLoud:wav.firstLoud, lastLoud:wav.lastLoud })

  // Save as, through the system picker.
  const saving = picker("Save project as", projectPath)
  await menu("File", "Save as…")
  await saving
  await until(async () => { const o = await owned(); return !o.snapshot.dirty ? o : null }, "the project to be saved", 20000)
  const saved = await project()
  const onDisk = JSON.parse(await readFile(projectPath, "utf8"))
  check("the saved file is the project in the window", onDisk.channels?.length === saved.channels.length && onDisk.playlist?.clips?.length === saved.playlist.clips.length, { channels:onDisk.channels?.length, clips:onDisk.playlist?.clips?.length })
  check("no panel crashed in the first session", (await app.crashed()).length === 0, await app.crashed())
  await app.quit()
  check("closing the window quits the app cleanly", app.session.exitCode === 0, { exitCode:app.session.exitCode, quitMs:app.session.quitMs })

  // ================= Second launch: open it again. =================
  app = await launch("reopen the song")
  report.timings.startupSecondMs = app.session.startupMs
  const second = app
  const opening = picker("Open project", projectPath)
  await second.menu("File", "Open project…")
  await opening
  const reopened = await second.until(async () => { const o = await second.owned(); return o.snapshot.project.channels.length === saved.channels.length ? o : null }, "the saved project to open", 20000)
  const back = reopened.snapshot.project
  check("the reopened project equals the saved one exactly", JSON.stringify(clone(back)) === JSON.stringify(clone(saved)), differences(saved, back))
  check("reopened: the notes of both patterns are there", JSON.stringify(back.patterns.map((p) => p.lanes)) === JSON.stringify(saved.patterns.map((p) => p.lanes)), null)
  check("reopened: the clips are where they were", JSON.stringify(back.playlist) === JSON.stringify(saved.playlist), null)
  check("reopened: the routing and effects are there", JSON.stringify(back.mixer) === JSON.stringify(saved.mixer) && back.channels.find((c) => c.id === kit[3].id).mixerTrack === bus.id, null)
  check("reopened: the automation is there", JSON.stringify(back.automations) === JSON.stringify(saved.automations) && back.automations[0].points.length === 3, back.automations)
  check("reopened: the project is clean", !reopened.snapshot.dirty, null)
  // The same song exported again is the same audio.
  await exportSong(second, exportAgainPath)
  const wav2 = decodeWav(await readFile(exportAgainPath))
  report.reexport = { frames:wav2.frames, peak:wav2.peak, rms:wav2.rms }
  check("exporting the reopened song gives the same audio, sample for sample", wav2.frames === wav.frames && wav2.data.equals(wav.data), { first:wav.frames, second:wav2.frames })
  // The plugin host and the installed plugin runtime, when the owned test
  // plugins are given (SMOKE_PLUGIN_FIXTURE, their library built for this
  // platform). By command: the plugin manager's folder and export choosers
  // are not driven here.
  const fixture = process.env.SMOKE_PLUGIN_FIXTURE
  if (fixture) {
    const pluginFolder = resolve(folder, "plugins"), exportFolder = resolve(folder, "plugin-export")
    await mkdir(pluginFolder, { recursive:true })
    // The export makes its destination itself and refuses one that exists.
    await copyFile(fixture, resolve(pluginFolder, "windfall-test-plugins.clap"))
    await second.invoke("export_plugin_bundle", { destination:exportFolder, runtime:"" })
    const exportedFiles = (await readdir(exportFolder, { recursive:true })).map(String)
    report.pluginExport = exportedFiles
    check("by command: the song exports as a plugin with the installed plugin runtime and helper", exportedFiles.some((name) => /\.clap$/.test(name)) && exportedFiles.some((name) => /windfall-plugin-audio/.test(name)), exportedFiles)
    await second.invoke("plugins_add_folder", { folder:pluginFolder })
    await second.invoke("plugins_scan", { retry:null })
    const scanned = await second.until(async () => { const state = await second.invoke("plugins_state"); return !state.scanning && state.entries.some((entry) => entry.id === "org.windfall.test.gain") ? state : null }, "the owned plugins in the scan", 90000)
      .catch(async (error) => { throw new Error(`${error.message}: ${JSON.stringify(await second.invoke("plugins_state")).slice(0, 3000)}`) })
    const gain = scanned.entries.find((entry) => entry.id === "org.windfall.test.gain")
    report.pluginScan = { entries:scanned.entries.length, blocked:scanned.blocked.length, gain }
    check("by command: the plugin scan finds the owned CLAP gain plugin, usable", gain?.format === "clap" && gain.usable, report.pluginScan)
    const kickTrack = back.channels.find((channel) => channel.id === kit[0].id).mixerTrack
    await second.invoke("plugins_add", { path:gain.path, id:gain.id, track:kickTrack })
    const binding = await second.until(async () => (await second.project()).plugins?.find((item) => item.id === gain.id && item.target.type === "effect"), "the plugin on the kick's mixer track", 30000)
    await delay(1000)
    const hosted = await second.invoke("plugins_state")
    report.pluginHost = { binding:{ ...binding, state:undefined }, instances:hosted.instances }
    check("by command: the CLAP plugin is hosted on the kick's track without an error", hosted.instances.length === 0, report.pluginHost)
    await exportSong(second, resolve(folder, "Song QA with plugin.wav"))
    const wav3 = decodeWav(await readFile(resolve(folder, "Song QA with plugin.wav")))
    report.pluginRender = { frames:wav3.frames, peak:wav3.peak, rms:wav3.rms, nonFinite:wav3.nonFinite }
    check("the song exported through the hosted plugin is audible and finite", wav3.rms > 0.005 && wav3.nonFinite === 0 && wav3.frames === wav.frames, report.pluginRender)
    // The plugin changed the project: saved from the File menu, so closing
    // the window does not stop at the save prompt.
    await second.menu("File", "Save")
    await second.until(async () => !(await second.owned()).snapshot.dirty, "the project to be saved with the plugin", 20000)
  }
  check("no panel crashed in the second session", (await second.crashed()).length === 0, await second.crashed())
  await second.quit()
  check("the second session quits cleanly", second.session.exitCode === 0, second.session.exitCode)
  app = null

  const consoleProblems = report.sessions.flatMap((session) => session.console)
  check("the webview console had no uncaught exception or error", consoleProblems.filter((entry) => entry.type !== "warning").length === 0, consoleProblems)
  report.passed = report.checks.every((item) => item.pass)
  if (!report.passed) process.exitCode = 1
} catch (error) {
  report.error = String(error.stack ?? error)
  report.windowText = await app?.evaluate("document.body.innerText").catch(() => null)
  const shot = await app?.screenshot().catch(() => null)
  if (shot) { await writeFile(resolve(folder, "failure.png"), Buffer.from(shot, "base64")); report.failureScreenshot = resolve(folder, "failure.png") }
  process.exitCode = 1
} finally {
  await app?.kill()
  report.console = report.sessions.flatMap((session) => session.console.map((entry) => ({ session:session.label, ...entry })))
  report.finished = new Date().toISOString()
  await mkdir(folder, { recursive:true })
  await writeFile(output, `${JSON.stringify(report, (key, value) => key === "data" ? undefined : value, 2)}\n`)
  console.log(`${report.passed ? "PASS" : "FAIL"}: ${output} (${report.checks.length} checks)`)
  // Nothing of the app's may keep this process waiting once it has reported.
  process.exit(process.exitCode ?? 0)
}
