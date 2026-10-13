import type { InstrumentAsset } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export type AssetRequest = { type: "audioSpectrum"; bytes: ArrayBuffer; rootHz: number } | { type: "imageSpectrum"; pixels: ArrayBuffer; width: number; height: number; color: boolean } | { type: "number"; number: number } | { type: "layer"; bytes: ArrayBuffer } | { type: "snapshots"; a: number[]; b: number[] }
/** Bounded worker-side WAV decoding; no WebAudio device or audio callback. */
export function prepareInstrumentAsset(request: AssetRequest): InstrumentAsset {
  if (request.type === "imageSpectrum") {
    const {width,height}=request
    if (!Number.isInteger(width)||!Number.isInteger(height)||width<1||width>256||height<1||height>64||request.pixels.byteLength!==width*height*4) throw new Error(t("Image preparation needs 1-256 columns and 1-64 rows of RGBA pixels"))
    const pixels=new Uint8Array(request.pixels)
    const frames=Array.from({length:width},(_,x)=>({partials:Array.from({length:height},(_,row)=>{
      const at=((height-1-row)*width+x)*4,r=pixels[at]!/255,g=pixels[at+1]!/255,b=pixels[at+2]!/255,alpha=pixels[at+3]!/255
      const brightness=(0.2126*r+0.7152*g+0.0722*b)*alpha,max=Math.max(r,g,b),min=Math.min(r,g,b),delta=max-min
      let hue=0
      if(delta>0) hue=(max===r?(g-b)/delta:max===g?2+(b-r)/delta:4+(r-g)/delta)/6
      hue=(hue+1)%1
      return {ratio:Math.fround(request.color?Math.pow(2,(row+2*hue)/12):row+1),level:Math.fround(brightness*brightness),phase:0,pan:Math.fround(request.color?b-r:0)}
    })}))
    return {type:"spectra",frames,frameSeconds:0.05,rootHz:440}
  }
  if (request.type === "audioSpectrum") {
    if (!Number.isFinite(request.rootHz)||request.rootHz<8||request.rootHz>20000) throw new Error(t("Source root pitch must be 8-20000 Hz"))
    const pcm=prepareInstrumentAsset({type:"layer",bytes:request.bytes})
    if(pcm.type!=="pcm") throw new Error(t("WAV preparation failed"))
    const {sampleRate,samples}=pcm,rootHz=request.rootHz
    const window=Math.min(samples.length,Math.max(32,Math.round(sampleRate/rootHz))),columns=Math.min(256,Math.max(1,Math.floor(samples.length/window)))
    const hop=columns===1?window:Math.max(window,Math.floor((samples.length-window)/(columns-1)))
    const frames=Array.from({length:columns},(_,column)=>({partials:Array.from({length:64},(_,i)=>{
      const ratio=i+1,hz=rootHz*ratio
      if(hz>=sampleRate*0.49) return {ratio,level:0,phase:0,pan:0}
      let re=0,im=0
      for(let j=0;j<window;j++) { const at=column*hop+j,angle=2*Math.PI*hz*at/sampleRate,x=samples[at]!/32767;re+=x*Math.cos(angle);im+=x*Math.sin(angle) }
      return {ratio,level:Math.fround(Math.min(1,2*Math.hypot(re,im)/window)),phase:Math.fround(((Math.atan2(re,im)/(2*Math.PI))%1+1)%1),pan:0}
    })}))
    return {type:"spectra",frames,frameSeconds:Math.max(0.001,hop/sampleRate),rootHz}
  }
  if (request.type === "number") {
    if (!Number.isInteger(request.number) || request.number<0 || request.number>4294967295) throw new Error(t("Patch number must be an integer from 0 to 4294967295"))
    let state=(request.number+0x9e3779b9)>>>0
    const draw=()=>{ state^=state<<13; state^=state>>>17; state^=state<<5; state>>>=0; return state/4294967295 }
    return {type:"spectra",frameSeconds:0.1,rootHz:440,presetNumber:request.number,frames:[{partials:Array.from({length:64},(_,i)=>{const n=i+1;return {ratio:Math.fround(n*(1+0.35*(draw()*2-1))),level:Math.fround((0.2+0.8*draw())/Math.pow(n,1.2)),phase:Math.fround(draw()),pan:0}})}]}
  }
  if (request.type === "snapshots") {
    if (request.a.length !== 32 || request.b.length !== 32 || [...request.a,...request.b].some(level => !Number.isFinite(level) || level<0 || level>1)) throw new Error(t("Snapshots need 32 harmonic levels from zero to one"))
    return { type: "spectra", frameSeconds: 0.1, rootHz: 440, frames: [request.a,request.b].map(levels => ({ partials: levels.map((level,index) => ({ ratio: index+1,level,phase: 0,pan: 0 })) })) }
  }
  const view = new DataView(request.bytes)
  const text = (at: number, count: number) => Array.from({ length: count }, (_, i) => String.fromCharCode(view.getUint8(at + i))).join("")
  if (view.byteLength < 44 || text(0,4) !== "RIFF" || text(8,4) !== "WAVE") throw new Error(t("Choose a PCM or float WAV file"))
  let format = 0, channels = 0, rate = 0, bits = 0, stride = 0, start = 0, length = 0
  for (let at = 12; at + 8 <= view.byteLength;) {
    const count = view.getUint32(at + 4,true), next = at + 8 + count
    if (next > view.byteLength) throw new Error(t("The WAV has a truncated chunk"))
    const tag = text(at,4)
    if (tag === "fmt ") { if (count < 16) throw new Error(t("The WAV format chunk is too short")); format = view.getUint16(at+8,true); channels = view.getUint16(at+10,true); rate = view.getUint32(at+12,true); stride = view.getUint16(at+20,true); bits = view.getUint16(at+22,true) }
    if (tag === "data") { start = at+8; length = count }
    at = next + (count % 2)
  }
  if (![1,3].includes(format) || channels < 1 || channels > 8 || rate < 8000 || rate > 192000 || ![16,24,32].includes(bits) || (format === 3 && bits !== 32) || stride !== channels * (bits/8) || length < stride) throw new Error(t("Supported WAV: 8–192 kHz, 1–8 channels, PCM 16/24/32-bit or float 32-bit"))
  const frames = Math.floor(length / stride), kept = Math.min(frames,32768)
  const samples = Array.from({ length: kept }, (_, frame) => {
    let sample = 0
    for (let channel = 0; channel < channels; channel++) {
      const at = start + frame*stride + channel*(bits/8)
      if (format === 3) sample += view.getFloat32(at,true)
      else if (bits === 16) sample += view.getInt16(at,true) / 32768
      else if (bits === 32) sample += view.getInt32(at,true) / 2147483648
      else { const raw = view.getUint8(at) | (view.getUint8(at+1)<<8) | (view.getUint8(at+2)<<16); sample += ((raw<<8)>>8)/8388608 }
    }
    sample /= channels
    const fade = frames > kept && frame >= kept - 64 ? (kept-frame)/64 : 1
    return Math.round(Math.max(-1,Math.min(1,Number.isFinite(sample) ? sample : 0)) * 32767 * fade)
  })
  return { type: "pcm", sampleRate: rate, samples }
}
