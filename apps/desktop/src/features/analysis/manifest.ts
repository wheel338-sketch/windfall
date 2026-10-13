import { decimal, type AnalysisModel, type AnalysisOnnxContract, type AnalysisTensorSpec } from "./types"
import { translatedText } from "@/lib/i18n"

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(translatedText("Expected a model manifest object."))
  return value as Record<string, unknown>
}
function text(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 1024)
    throw new Error(translatedText("Model fields require bounded nonempty strings."))
  return value
}
function count(value: unknown): string {
  const result = text(value)
  if (!decimal(result) || result === "0")
    throw new Error(translatedText("Model counts require positive canonical decimal strings."))
  return result
}
function integer(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value <= 0 ||
    value > 384000
  )
    throw new Error(translatedText("Invalid model audio shape."))
  return value
}
/** Parsing retains only bounded canonical manifest fields, never JSON extras. */
export function parseManifest(json: string): AnalysisModel {
  if (json.length > 16384) throw new Error(translatedText("Model manifest limit is 16 KiB."))
  const m = record(JSON.parse(json) as unknown)
  const p = record(m.provenance)
  const sha256 = text(m.sha256)
  if (!/^[0-9a-f]{64}$/.test(sha256))
    throw new Error(translatedText("Expected an explicit lowercase SHA256."))
  if (
    !Array.isArray(m.outputs) ||
    m.outputs.length === 0 ||
    m.outputs.length > 16
  )
    throw new Error(translatedText("Declare one to sixteen output roles."))
  const result: AnalysisModel = {
    ...(m.onnx == null ? {} : { onnx: onnx(m.onnx) }),
    id: text(m.id),
    version: text(m.version),
    revision: count(m.revision),
    sha256,
    bytes: count(m.bytes),
    maxBytes: count(m.maxBytes),
    sampleRate: integer(m.sampleRate),
    inputChannels: integer(m.inputChannels),
    maxInputFrames: count(m.maxInputFrames),
    provenance: {
      origin: text(p.origin),
      sourceRevision: text(p.sourceRevision),
      author: text(p.author),
      licenseSpdx: text(p.licenseSpdx),
      licenseReference: text(p.licenseReference),
      adapterId: text(p.adapterId),
      adapterVersion: text(p.adapterVersion),
      device: text(p.device),
    },
    outputs: m.outputs.map((output: unknown) => {
      const o = record(output)
      return { role: text(o.role), channels: integer(o.channels) }
    }),
  }
  if (result.provenance.adapterId === "windfall-onnx-separation") validateSeparation(result)
  return result
}

function tensors(value: unknown): AnalysisTensorSpec[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > 16)
    throw new Error(translatedText("Declare one to sixteen named ONNX tensors."))
  return value.map((item: unknown) => {
    const t = record(item)
    if (!Array.isArray(t.shape) || t.shape.length < 1 || t.shape.length > 8)
      throw new Error(translatedText("ONNX tensors need one to eight explicit dimensions."))
    return { name: text(t.name), shape: t.shape.map((d: unknown) => {
      if (typeof d === "number" && Number.isInteger(d) && d > 0 && d <= 16777216) return d
      if (typeof d === "string" && /^[A-Za-z0-9_]{1,64}$/.test(d)) return d
      throw new Error(translatedText("Use positive dimensions or bounded symbolic names."))
    }) }
  })
}
function onnx(value: unknown): AnalysisOnnxContract {
  const o = record(value)
  if (!Array.isArray(o.stemRoles) || o.stemRoles.length < 1 || o.stemRoles.length > 16)
    throw new Error(translatedText("Declare the ONNX stem dimension order."))
  if (typeof o.overlapFrames !== "number" || !Number.isSafeInteger(o.overlapFrames) || o.overlapFrames < 0)
    throw new Error(translatedText("ONNX overlap must be a nonnegative frame count."))
  if (typeof o.segmentFrames !== "number" || !Number.isSafeInteger(o.segmentFrames) || o.segmentFrames <= o.overlapFrames || o.segmentFrames > 16777216)
    throw new Error(translatedText("ONNX segment must exceed overlap."))
  return { opset: integer(o.opset), inputs: tensors(o.inputs), outputs: tensors(o.outputs),
    ...(o.layout == null ? {} : { layout: layout(o.layout) }),
    ...(o.contextFrames == null ? {} : { contextFrames: context(o.contextFrames) }),
    sampleRate: integer(o.sampleRate), segmentFrames: o.segmentFrames,
    overlapFrames: o.overlapFrames, stemRoles: o.stemRoles.map(text) }
}
function layout(value: unknown): "channelsFirst" | "channelsLast" {
  if (value !== "channelsFirst" && value !== "channelsLast") throw new Error(translatedText("Use a waveform channelsFirst or channelsLast layout."))
  return value
}
function context(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0 || value > 1536000)
    throw new Error(translatedText("Invalid required context frames."))
  return value
}
function validateSeparation(model: AnalysisModel) {
  const spec = model.onnx
  const roles = ["drums", "bass", "vocals", "other"]
  if (!spec || spec.inputs.length !== 1 || spec.outputs.length !== 1 || spec.stemRoles.length !== 4 ||
    new Set(spec.stemRoles).size !== 4 || roles.some((r) => !spec.stemRoles.includes(r)) ||
    model.outputs.length !== 4 || new Set(model.outputs.map((o) => o.role)).size !== 4 ||
    roles.some((r) => !model.outputs.some((o) => o.role === r)) ||
    model.outputs.some((o) => o.channels !== model.inputChannels) ||
    ![1, 2].includes(model.inputChannels) || model.sampleRate < 8000 || model.sampleRate > 192000 ||
    spec.sampleRate !== model.sampleRate || spec.opset > 64)
    throw new Error(translatedText("Separation requires one waveform input/output and exactly four aligned stem roles."))
  const context = spec.contextFrames ?? 0
  const frames = spec.segmentFrames + context * 2
  if (frames > 1536000 || context > spec.segmentFrames || spec.segmentFrames > BigInt(model.maxInputFrames) ||
    spec.segmentFrames - spec.overlapFrames < Math.floor(spec.segmentFrames / 4))
    throw new Error(translatedText("Separation chunk/context/overlap exceeds the bounded contract."))
  const last = spec.layout === "channelsLast"
  const c = model.inputChannels
  for (const [tensor, expected] of [[spec.inputs[0], last ? [1, frames, c] : [1, c, frames]],
    [spec.outputs[0], last ? [1, 4, frames, c] : [1, 4, c, frames]]] as const) {
    const timeAxis = expected.length - (last ? 2 : 1)
    if (tensor.name.length > 128 || Array.from(tensor.name).some(char => char.charCodeAt(0) <= 31 || char.charCodeAt(0) === 127) || tensor.shape.length !== expected.length ||
      tensor.shape.some((d, i) => typeof d === "number" ? d !== expected[i] : d !== "frames" || i !== timeAxis))
      throw new Error(translatedText("Invalid waveform tensor shape convention."))
  }
}
