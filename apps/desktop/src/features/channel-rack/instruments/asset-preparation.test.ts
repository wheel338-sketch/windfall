import { describe, expect, it } from "vitest"
import { prepareInstrumentAsset } from "./asset-preparation"
function wav(samples: number[], rate = 48000) {
  const bytes = new ArrayBuffer(44 + samples.length*2), view = new DataView(bytes)
  for (const [at,text] of [[0,"RIFF"],[8,"WAVE"],[12,"fmt "],[36,"data"]] as const) for (let i=0;i<text.length;i++) view.setUint8(at+i,text.charCodeAt(i))
  view.setUint32(4,bytes.byteLength-8,true); view.setUint32(16,16,true); view.setUint16(20,1,true); view.setUint16(22,1,true); view.setUint32(24,rate,true); view.setUint32(28,rate*2,true); view.setUint16(32,2,true); view.setUint16(34,16,true); view.setUint32(40,samples.length*2,true)
  samples.forEach((sample,i) => view.setInt16(44+i*2,sample,true)); return bytes
}
describe("instrument worker WAV preparation", () => {
  it("decodes PCM independently and preserves the source rate", () => {
    const asset = prepareInstrumentAsset({ type: "layer",bytes: wav([0,8192,-16384,24576],44100) })
    expect(asset).toEqual({ type: "pcm",sampleRate: 44100,samples: [0,8192,-16383,24575] })
  })
  it("bounds and fades long layers, and rejects malformed/unsupported input", () => {
    const asset = prepareInstrumentAsset({ type: "layer",bytes: wav(Array.from({length:40000},() => 32767)) })
    if (asset.type !== "pcm") throw new Error("wrong asset")
    expect(asset.samples).toHaveLength(32768); expect(asset.samples.at(-1)).toBeLessThan(520)
    expect(() => prepareInstrumentAsset({ type: "layer",bytes: new ArrayBuffer(5) })).toThrow("WAV")
    expect(() => prepareInstrumentAsset({ type: "layer",bytes: wav([100],4000) })).toThrow("Supported WAV")
  })
})

it("prepares independent immutable harmonic snapshots and refuses invalid levels", () => {
  const a=Array.from({length:32},(_,i) => i===0 ? 1 : 0),b=Array.from({length:32},(_,i) => 1/(i+1))
  const asset=prepareInstrumentAsset({type:"snapshots",a,b})
  if(asset.type!=="spectra") throw new Error("wrong asset")
  expect(asset.frames[0].partials[4]).toEqual({ratio:5,level:0,phase:0,pan:0})
  expect(asset.frames[1].partials[4].level).toBe(0.2)
  expect(() => prepareInstrumentAsset({type:"snapshots",a:[-1],b})).toThrow("Snapshots")
})

it("derives deterministic patches across the full unsigned number domain",()=>{
  for(const number of [0,1,16777216,2147483648,4294967295]) { const asset=prepareInstrumentAsset({type:"number",number});expect(asset).toEqual(prepareInstrumentAsset({type:"number",number}));expect(asset).toMatchObject({type:"spectra",presetNumber:number});expect(asset).not.toEqual(prepareInstrumentAsset({type:"number",number:number===0?1:0})); }
  for(const number of [-1,1.5,4294967296,NaN]) expect(()=>prepareInstrumentAsset({type:"number",number})).toThrow(/Patch number/)
})

it("reconstructs a coherent harmonic WAV within a measured error bound",()=>{
  const samples=Array.from({length:1000},(_,i)=>Math.round(32767*(0.4*Math.sin(2*Math.PI*i/100+0.7)+0.2*Math.sin(2*Math.PI*3*i/100+1.1))))
  const asset=prepareInstrumentAsset({type:"audioSpectrum",bytes:wav(samples),rootHz:480})
  if(asset.type!=="spectra")throw new Error("wrong asset")
  expect(asset.frames).toHaveLength(10)
  const partials=asset.frames[0]!.partials
  expect(partials[0]!.level).toBeCloseTo(0.4,3);expect(partials[2]!.level).toBeCloseTo(0.2,3)
  const mse=samples.slice(0,100).reduce((error,x,i)=>{const y=partials.reduce((sum,p)=>sum+p.level*Math.sin(2*Math.PI*(p.ratio*i/100+p.phase)),0);return error+(y-x/32767)**2},0)/100
  expect(Math.sqrt(mse)).toBeLessThan(0.001)
  expect(()=>prepareInstrumentAsset({type:"audioSpectrum",bytes:wav(samples),rootHz:NaN})).toThrow("root pitch")
})
it("maps image columns, reversed rows, alpha and luminance independently",()=>{
  const pixels=new Uint8Array([255,255,255,255,0,0,0,255,128,128,128,255,255,255,255,0]).buffer
  const asset=prepareInstrumentAsset({type:"imageSpectrum",pixels,width:2,height:2,color:false})
  if(asset.type!=="spectra")throw new Error("wrong asset")
  expect(asset.frames[0]!.partials[0]!.level).toBeCloseTo((128/255)**2,6)
  expect(asset.frames[0]!.partials[1]).toEqual({ratio:2,level:1,phase:0,pan:0})
  expect(asset.frames[1]!.partials.map(p=>p.level)).toEqual([0,0])
  expect(()=>prepareInstrumentAsset({type:"imageSpectrum",pixels,width:257,height:2,color:false})).toThrow("RGBA")
})

it("maps red and blue hue to pitch, squared brightness and stereo scan coordinates",()=>{
  const asset=prepareInstrumentAsset({type:"imageSpectrum",pixels:new Uint8Array([255,0,0,255,0,0,255,255]).buffer,width:2,height:1,color:true})
  if(asset.type!=="spectra")throw new Error("wrong asset")
  expect(asset.frames[0]!.partials[0]).toEqual({ratio:1,level:Math.fround(0.2126**2),phase:0,pan:-1})
  expect(asset.frames[1]!.partials[0]!.ratio).toBeCloseTo(2**((4/3)/12),6)
  expect(asset.frames[1]!.partials[0]!.level).toBeCloseTo(0.0722**2,6)
  expect(asset.frames[1]!.partials[0]!.pan).toBe(1)
})
