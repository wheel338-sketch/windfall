// Node 22+, Windows WebView2, prebuilt debug app. No packages or framework.
import { createServer } from "node:http"
import { spawn } from "node:child_process"
import { mkdir, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const executable = resolve(process.argv[2] ?? "target/debug/windfall-desktop.exe")
const output = resolve(process.argv[3] ?? "target/notebook-window/report.json")
const port = Number(process.argv[4] ?? 9259)
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
const report = { schema:1, executable, config, outputGain:0, started:new Date().toISOString(), fixture:null, samples:[], checks:[], passed:false }
let socket
let requests = 0
const server = createServer((request, response) => { requests++; response.writeHead(200); response.end("hostile resource") })
await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve) })
const attackUrl = `http://127.0.0.1:${server.address().port}/hostile`
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
  const cdp = (method, params, sessionId) => new Promise((resolve, reject) => {
    const id = ++next
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)) }, 15000)
    pending.set(id, { resolve, reject, timer })
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }))
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
    try {
      report.timeoutFrameTree = await cdp('Page.getFrameTree', {})
      report.timeoutTargets = await cdp('Target.getTargets', {})
      const root = await cdp('DOM.getDocument', {})
      const owner = await cdp('DOM.querySelector', { nodeId: root.root.nodeId, selector: 'iframe[title="Notebook HTML preview"]' })
      if (owner.nodeId) report.timeoutFrameOwner = await cdp('DOM.describeNode', { nodeId: owner.nodeId })
      report.timeoutFrame = await evaluate(`(() => {const frame=document.querySelector('iframe[title="Notebook HTML preview"]');return frame?{src:frame.src,sandbox:frame.getAttribute('sandbox'),width:frame.getBoundingClientRect().width,height:frame.getBoundingClientRect().height}:null})()`)
      report.timeoutDom = await evaluate('document.body.innerText.slice(-12000)')
    } catch { /* Retain the original assertion failure. */ }
    throw new Error(`Timed out waiting for ${what}`)
  }
  const invoke = (command, args = {}) => evaluate(`(async () => {
    const command = ${JSON.stringify(command)};
    const args = ${JSON.stringify(args)};
    if (command === "window_document_snapshot" || command === "document_snapshot") return window.__TAURI_INTERNALS__.invoke(command, args);
    const owned = await window.__TAURI_INTERNALS__.invoke("window_document_snapshot");
    return window.__TAURI_INTERNALS__.invoke(command, { ...args, caller: args.caller ?? owned.caller });
  })()`)
  await until(() => evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function" && Boolean(document.querySelector("[data-channel-button]"))'), 'the real main desktop rack')
  // Tauri exposes this callback map for debugging. Observe the actual channel
  // payload while preserving its original callback and main-window ownership.
  await evaluate(`(() => {const callbacks=window.__TAURI_INTERNALS__.callbacks;const set=callbacks.set.bind(callbacks);window.__r3Frames=[];const wrap=callback=>data=>{const frame=data?.message;if(frame&&Array.isArray(frame.meters)&&typeof frame.voices==="number"){window.__r3Frames.push(frame);if(window.__r3Frames.length>120)window.__r3Frames.shift()}return callback(data)};for(const [id,callback]of callbacks)set(id,wrap(callback));callbacks.set=(id,callback)=>set(id,wrap(callback));return true})()`)
  const owned = () => invoke('window_document_snapshot')
  const dispatch = async (command) => invoke('dispatch', { command, generation:(await owned()).generation })
  const check = (name, pass, detail) => { report.checks.push({name, pass:Boolean(pass),detail}); if (!pass) throw new Error(`Failed: ${name}`) }
  const point = async (selector, text) => until(() => evaluate(`(() => { const element=[...document.querySelectorAll(${JSON.stringify(selector)})].find(e=>(${text === undefined ? 'true' : `e.textContent.trim()===${JSON.stringify(text)}`})&&e.getClientRects().length>0&&getComputedStyle(e).visibility!=="hidden"); if(!element || element.disabled)return null; element.scrollIntoView({block:"center"}); const r=element.getBoundingClientRect(); return r.width&&r.height?{x:r.x+r.width/2,y:r.y+r.height/2}:null })()`), `visible ${selector} ${text??''}`)
  const click = async (selector, text) => {
    const p=await point(selector,text)
    await cdp('Input.dispatchMouseEvent',{type:'mouseMoved',...p})
    await cdp('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'left',clickCount:1})
    await cdp('Input.dispatchMouseEvent',{type:'mouseReleased',...p,button:'left',clickCount:1})
  }
  const key = async (value, code=value, modifiers=0) => {
    const windowsVirtualKeyCode={a:65,Tab:9,Enter:13,End:35}[value]??0
    await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:value,code,modifiers,windowsVirtualKeyCode})
    await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:value,code,modifiers,windowsVirtualKeyCode})
  }
  const pick = async (label,text) => {
    await click(`[role="combobox"][aria-label="${label}"]`)
    await click('[role="option"]',text)
    await until(()=>evaluate(`Array.from(document.querySelectorAll('[role="listbox"]')).every(element=>element.getClientRects().length===0||getComputedStyle(element).visibility==='hidden')`),'the source popup to finish closing')
  }
  const number = async (label,value) => {
    await click(`input[aria-label="${label}"]`)
    await key('a','KeyA',2)
    await cdp('Input.insertText',{text:String(value)})
    await key('Tab','Tab')
  }

  // DevTools can expose the target while its initial about:blank is still
  // navigating. Read the live document rather than that stale target record.
  const packagedUrl = await until(async () => {
    const url = await evaluate('location.href')
    const parsed = new URL(url)
    return ((['http:', 'https:'].includes(parsed.protocol) && parsed.hostname === 'tauri.localhost') ||
      (parsed.protocol === 'tauri:' && parsed.hostname === 'localhost')) ? url : null
  }, 'the packaged application origin')
  const origin = new URL(packagedUrl)
  check('packaged application origin',
    (['http:', 'https:'].includes(origin.protocol) && origin.hostname === 'tauri.localhost') ||
    (origin.protocol === 'tauri:' && origin.hostname === 'localhost'), packagedUrl)
  const before = (await owned()).snapshot
  const nonce = 'windfall-notebook-hostile-r4'
  await evaluate(`(() => { window.__notebookAttackCount = 0; window.addEventListener('message', event => { if(event.data === ${JSON.stringify(nonce)}) window.__notebookAttackCount++ }); return true })()`)
  await click('[role="tab"]', 'Playlist')
  await click('button', 'Notebook')
  await click('label', 'Safe HTML preview')
  const fixtures = [
    `<h2>Visible note</h2><script>parent.postMessage('${nonce}','*');parent.document.body.textContent='owned';window.__TAURI_INTERNALS__.invoke('project_new')</script>`,
    `<p>Visible note</p><img src="${attackUrl}" onerror="parent.postMessage('${nonce}','*')"><style>@import '${attackUrl}';</style><iframe src="${attackUrl}"></iframe><form action="${attackUrl}"><input></form>`,
    `<p>Visible note</p><object data="file:///C:/Windows/win.ini"></object><img src="asset://localhost/C:/Windows/win.ini"><iframe src="tauri://localhost/index.html"></iframe><img src="http://ipc.localhost/project_new">`,
    `<p>Visible note</p><svg><foreignObject><script>parent.postMessage('${nonce}','*')</script></foreignObject></svg><meta http-equiv="refresh" content="0;url=${attackUrl}"><base href="${attackUrl}"><iframe srcdoc="&lt;script&gt;parent.postMessage('${nonce}','*')&lt;/script&gt;"></iframe>`,
  ]
  let lastSource
  for (const [index, source] of fixtures.entries()) {
    const stateBefore = (await owned()).snapshot
    await click('textarea')
    await key('a', 'KeyA', 2)
    await cdp('Input.insertText', { text:source })
    await until(() => evaluate(`(() => {const frame=document.querySelector('iframe[title="Notebook HTML preview"]');if(!frame?.src.startsWith('data:'))return false;const delivered=decodeURIComponent(frame.src.slice(frame.src.indexOf(',')+1));return delivered.includes('Visible note')&&!delivered.includes('<script')})()`), 'the inert native notebook document')
    const previewUrl = await evaluate(`document.querySelector('iframe[title="Notebook HTML preview"]').src`)
    const navigation = await until(async () => {
      const root = await cdp('DOM.getDocument', {})
      const found = await cdp('DOM.querySelector', { nodeId: root.root.nodeId, selector: 'iframe[title="Notebook HTML preview"]' })
      if (!found.nodeId) return null
      const owner = await cdp('DOM.describeNode', { nodeId: found.nodeId })
      const frameId = owner.node.frameId
      const tree = await cdp('Page.getFrameTree', {})
      const contains = (node) => (node.frame.id === frameId && node.frame.url === previewUrl) || (node.childFrames ?? []).some(contains)
      if (contains(tree.frameTree)) return { frameId }
      // WebView2 isolates this opaque iframe in a different renderer. Its
      // target belongs to this exact DOM frame owner and packaged parent.
      const targets = await cdp('Target.getTargets', {})
      const target = targets.targetInfos.find(info => info.type === 'iframe' && info.targetId === frameId &&
        info.parentFrameId === tree.frameTree.frame.id && info.url === previewUrl)
      return target ? { frameId, targetId: target.targetId } : null
    }, 'the actual packaged iframe navigation')
    let sessionId
    let contextId
    try {
      if (navigation.targetId) {
        sessionId = (await cdp('Target.attachToTarget', { targetId: navigation.targetId, flatten: true })).sessionId
      } else {
        contextId = (await cdp('Page.createIsolatedWorld', { frameId: navigation.frameId, worldName: 'notebook-evidence', grantUniversalAccess: false })).executionContextId
      }
      const visible = await until(async () => {
        const result = await cdp('Runtime.evaluate', { expression: 'document.body?.innerText.trim()', returnByValue: true,
          ...(contextId ? { contextId } : {}) }, sessionId)
        return !result.exceptionDetails && result.result.value === 'Visible note'
      }, 'the rendered authored notebook text')
      check(`fixture ${index}: authored formatting actually renders`, visible, navigation)
    } finally {
      if (sessionId) await cdp('Target.detachFromTarget', { sessionId })
    }
    const frame = await evaluate(`(() => {const f=document.querySelector('iframe[title="Notebook HTML preview"]');let parentReadDenied=false;try {void f.contentWindow.document}catch(error){parentReadDenied=error.name==='SecurityError'}const delivered=decodeURIComponent(f.src.slice(f.src.indexOf(',')+1));return {sandbox:f.getAttribute('sandbox'),srcdoc:f.hasAttribute('srcdoc'),origin:new URL(f.src).origin,parentReadDenied,contentDocumentNull:f.contentDocument===null,referrer:f.referrerPolicy,delivered,witness:window.__notebookAttackCount}})()`)
    check(`fixture ${index}: opaque native frame`, frame.origin==='null' && frame.parentReadDenied && frame.contentDocumentNull, frame)
    check(`fixture ${index}: permissionless sandbox`, frame.sandbox==='' && !frame.srcdoc && frame.referrer==='no-referrer', frame)
    check(`fixture ${index}: resource and IPC denial`, frame.delivered.includes("connect-src 'none'") && frame.delivered.includes("script-src 'none'") && !/<script|<img|<iframe|<object|<svg|file:\/\/|asset:\/\/|ipc\.localhost|__TAURI_INTERNALS__/.test(frame.delivered), frame.delivered)
    check(`fixture ${index}: no authored script or network`, frame.witness===0 && requests===0, {witness:frame.witness, requests})
    const unchanged = (await owned()).snapshot
    check(`fixture ${index}: no native document mutation from draft or attack`, JSON.stringify(unchanged)===JSON.stringify(stateBefore), {revision:unchanged.revision, before:stateBefore.revision})
    await click('button', 'Save notebook')
    await until(async () => (await owned()).snapshot.project.notebook?.pages[0]?.body===source, 'the checked notebook edit')
    const saved = (await owned()).snapshot
    check(`fixture ${index}: exact authored source persisted through checked edit`, saved.revision===stateBefore.revision+1 && saved.project.notebook.pages[0].html===true && saved.project.notebook.pages[0].body===source, {revision:saved.revision})
    lastSource=source
  }
  const projectFile=resolve(config, 'hostile.windfall')
  await invoke('undo', {generation:(await owned()).generation})
  check('hostile notebook edit has real native Undo', (await owned()).snapshot.project.notebook.pages[0].body===fixtures[fixtures.length-2], null)
  await invoke('redo', {generation:(await owned()).generation})
  check('hostile notebook edit has real native Redo', (await owned()).snapshot.project.notebook.pages[0].body===lastSource, null)
  await invoke('project_save', {path:projectFile})
  await invoke('project_new')
  await invoke('project_open', {path:projectFile})
  const reopened=(await owned()).snapshot
  check('hostile source survives native save/reopen', reopened.project.notebook.pages[0].body===lastSource && reopened.project.notebook.pages[0].html===true && !reopened.dirty, reopened.project.notebook)
  // Document replacement correctly closes transient editors. Reopen through
  // the persisted project's real toolbar before inspecting its preview.
  await click('button', 'Notebook')
  await until(() => evaluate(`Boolean(document.querySelector('iframe[title="Notebook HTML preview"]')?.src.startsWith('data:'))`), 'the reopened isolated notebook preview')
  check('reopened native preview still refuses parent access', await evaluate(`(() => {const f=document.querySelector('iframe[title="Notebook HTML preview"]');try {void f.contentWindow.document;return false}catch(error){return error.name==='SecurityError'&&f.contentDocument===null&&f.getAttribute('sandbox')===''}})()`), null)
  check('no notebook network request across the complete packaged run', requests===0, {requests})
  check('native authored content did not replace the project', reopened.project.settings.name===before.project.settings.name, reopened.project.settings.name)
  report.passed=true
} catch(error) {
  report.error=String(error?.stack??error)
  process.exitCode=1
} finally {
  socket?.close()
  child.kill()
  await new Promise(resolve => server.close(resolve))
  report.logs=logs
  report.finished=new Date().toISOString()
  await mkdir(dirname(output), {recursive:true})
  await writeFile(output, JSON.stringify(report,null,2))
  console.log(JSON.stringify({output,passed:report.passed,checks:report.checks.length,error:report.error}))
}
