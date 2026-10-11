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
