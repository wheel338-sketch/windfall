// Node 22+, Windows WebView2, prebuilt debug app. No packages or framework.
import { spawn } from "node:child_process"
import { mkdir, writeFile, readFile, unlink } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { setTimeout as delay } from "node:timers/promises"

const executable = resolve(process.argv[2] ?? "target/debug/windfall-desktop.exe")
const output = resolve(process.argv[3] ?? "target/sample-window/report.json")
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
  env:{ ...process.env, WINDFALL_ANALYZER_SMOKE:"0", WINDFALL_CONFIG_DIR:config,
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
  const wav = (hz,phase) => {
    const frames=96000, data=Buffer.alloc(frames*8), out=Buffer.alloc(44+data.length)
    for(let f=0;f<frames;f++)for(let c=0;c<2;c++)data.writeFloatLE(0.3*Math.sin(2*Math.PI*(hz+c*phase)*f/48000)*Math.min(1,(f%12000)/400,(12000-f%12000)/400),f*8+c*4)
    out.write('RIFF');out.writeUInt32LE(out.length-8,4);out.write('WAVEfmt ',8);out.writeUInt32LE(16,16);out.writeUInt16LE(3,20);out.writeUInt16LE(2,22);out.writeUInt32LE(48000,24);out.writeUInt32LE(384000,28);out.writeUInt16LE(8,32);out.writeUInt16LE(32,34);out.write('data',36);out.writeUInt32LE(data.length,40);data.copy(out,44);return out
  }
  const sourceA=resolve(dirname(output),'r3-a.wav'), sourceB=resolve(dirname(output),'r3-b.wav')
  const originalA=wav(440,0),originalB=wav(880,40)
  await writeFile(sourceA,originalA);await writeFile(sourceB,originalB)
  report.fixture={sourceA,sourceB,rate:48000,frames:96000}
  const pcm = (buffer) => {
    if(buffer.toString('ascii',0,4)!=='RIFF')throw new Error('Native export is not a WAV')
    for(let at=12;at+8<=buffer.length;){const length=buffer.readUInt32LE(at+4);if(buffer.toString('ascii',at,at+4)==='data')return buffer.subarray(at+8,at+8+length);at+=8+length+(length%2)}
    throw new Error('Native export has no data chunk')
  }
  const audible = (data) => {let energy=0;for(let at=0;at<data.length;at+=4){const sample=data.readFloatLE(at);if(!Number.isFinite(sample))return false;energy+=sample*sample}return energy/data.length>1e-8}
  for(const kind of ['sliceMap','sliceDeck','grainCloud','waveRide']) {
    await invoke('project_new')
    for(const channel of (await owned()).snapshot.project.channels)await dispatch({type:'updateChannel',id:channel.id,patch:{muted:true}})
    await invoke('add_channel_from_file',{path:sourceA,index:null,browser:null})
    await invoke('add_channel_from_file',{path:sourceB,index:null,browser:null})
    const imported=(await owned()).snapshot.project
    const a=imported.samples.find(s=>s.path.path===sourceA||s.name==='r3-a'), b=imported.samples.find(s=>s.path.path===sourceB||s.name==='r3-b')
    check(`${kind}: real native imported sources`,a&&b,{a,b})
    const created=await dispatch({type:'addChannel',instrument:kind,name:`R3 ${kind}`}), id=created.created[0]
    await click(`[data-channel-button="${id}"]`)
    await pick('Source A',a.name)
    const channel = async () => (await owned()).snapshot.project.channels.find(c=>c.id===id)
    await until(async()=> (await channel()).samplePlayback?.sample===a.id,`${kind} source selection`)
    if(kind==='sliceDeck'){await pick('Source B',b.name);await until(async()=> (await channel()).samplePlayback?.sampleB===b.id,'deck B selection')}
    const pattern=(await owned()).snapshot.project.patterns[0].id
    if(kind==='sliceMap'||kind==='sliceDeck'){
      await click('button','Detect beat grid')
      await until(async()=> (await channel()).samplePlayback.slices.length>=4,'native detected grid')
      await click('button','Generate slice pattern')
      await until(async()=> (await owned()).snapshot.project.patterns[0].lanes.some(l=>l.channel===id&&l.notes.length>=4),'mapped pattern notes')
    }else await dispatch({type:'addNotes',pattern,channel:id,notes:[{start:0,length:1920,key:60,velocity:0.8}]})
    await invoke('transport_set',{patch:{mode:'pattern',pattern}})
    const exportPcm = async (name) => {
      const path=resolve(dirname(output),`${kind}-${name}.wav`)
      try{await unlink(path)}catch(error){if(error.code!=='ENOENT')throw error}
      await invoke('export_audio',{options:{path,format:'wav',bitDepth:'float32',sampleRate:48000,mode:'pattern',patternLoops:1,tailSecs:0,autoTail:false}})
      const buffer=await until(async()=>{try{return await readFile(path)}catch(error){if(error.code==='ENOENT')return null;throw error}},'native export publication')
      const data=pcm(buffer);check(`${kind}: ${name} finite audible native export`,audible(data),{path,bytes:data.length});return data
    }
    const before=await channel(), cursor=(await owned()).snapshot.history.cursor, baseline=await exportPcm('before')
    if(kind==='grainCloud'){await click('[role="slider"][aria-label="Density"]');await key('End','End')}
    else if(kind==='waveRide')await number('Point 2 value',0.2)
    else if(kind==='sliceDeck')await number('Slice deck',1)
    else await click('button','Split slice')
    await until(async()=> (await owned()).snapshot.history.cursor===cursor+1,'one authoring history step')
    const edited=await channel()
    check(`${kind}: actual editor changed native document`,JSON.stringify(edited)!==JSON.stringify(before),edited)
    const changed=await exportPcm('edited');check(`${kind}: authoring changes actual PCM`,!changed.equals(baseline),{bytes:changed.length})
    await invoke('undo',{generation:(await owned()).generation})
    check(`${kind}: native Undo restores exact channel`,JSON.stringify(await channel())===JSON.stringify(before),await channel())
    check(`${kind}: native Undo restores exact PCM`,(await exportPcm('undo')).equals(baseline),{})
    await invoke('redo',{generation:(await owned()).generation})
    check(`${kind}: native Redo restores exact channel`,JSON.stringify(await channel())===JSON.stringify(edited),await channel())
    check(`${kind}: native Redo restores exact PCM`,(await exportPcm('redo')).equals(changed),{})
    const meter = () => evaluate(`(() => {const canvas=document.querySelector('[data-slot="master-meter"] canvas');if(!canvas)return null;const context=canvas.getContext('2d');if(!context)return null;const data=context.getImageData(0,0,canvas.width,canvas.height).data;const colors=new Map();let opaque=0;for(let i=0;i<data.length;i+=4)if(data[i+3]){opaque++;const color=[data[i],data[i+1],data[i+2],data[i+3]].join(',');colors.set(color,(colors.get(color)||0)+1)}return {width:canvas.width,height:canvas.height,signalPixels:opaque-Math.max(0,...colors.values())}})()`)
    const idleMeter=await meter()
    await click('[role="button"][aria-label="C5"],button[aria-label="C5"]')
    await evaluate('window.__r3Frames=[]')
    await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter'})
    await delay(200)
    await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter'})
    const soundingMeter=await meter()
    check(`${kind}: native keyboard produces actual live PCM in the real master meter`,idleMeter&&soundingMeter&&soundingMeter.signalPixels>idleMeter.signalPixels,{idleMeter,soundingMeter})
    const soundingFrames=await evaluate('window.__r3Frames')
    check(`${kind}: live native keyboard frame has sounding voices and PCM`,soundingFrames.some(frame=>frame.voices>0&&frame.meters.slice(0,2).some(peak=>peak>0)),soundingFrames.map(frame=>({voices:frame.voices,master:frame.meters.slice(0,2)})))
    await evaluate('window.__r3Frames=[]')
    await delay(1000)
    const releasedFrames=await evaluate('window.__r3Frames')
    check(`${kind}: native keyboard release leaves no voices held`,releasedFrames.length>0&&releasedFrames.at(-1).voices===0,releasedFrames.map(frame=>frame.voices))
    const path=resolve(dirname(output),`${kind}.windfall`)
    await invoke('project_save',{path});await invoke('project_new');await invoke('project_open',{path})
    await until(async()=>JSON.stringify(await channel())===JSON.stringify(edited),'saved native channel reopen')
    await click(`[data-channel-button="${id}"]`)
    await until(()=>evaluate(`Boolean(document.querySelector('[aria-label="Source A"]'))`),'reopened actual editor')
    await invoke('transport_set',{patch:{mode:'pattern',pattern}})
    check(`${kind}: reopened exact native PCM`,(await exportPcm('reopened')).equals(changed),{})
    report.samples.push({kind,id,project:path,channel:await channel()})
  }
  check('original source PCM files remain exact',(await readFile(sourceA)).equals(originalA)&&(await readFile(sourceB)).equals(originalB),report.fixture)
  const svg=await readFile('docs/qa/p2-piano-artifacts/known-phrase.svg','utf8')
  await evaluate(`(() => {document.body.innerHTML=${JSON.stringify(svg)};document.body.style.background='white';document.body.style.color='black';return true})()`)
  const scoreImage=await cdp('Page.captureScreenshot',{format:'png',captureBeyondViewport:true})
  report.scoreSvgImage=resolve('target/round3-score-svg.png')
  await writeFile(report.scoreSvgImage,Buffer.from(scoreImage.data,'base64'))
  report.passed=report.checks.every(c=>c.pass)

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
