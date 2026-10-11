import { decimal, type AnalysisModel, type AnalysisOnnxContract, type AnalysisTensorSpec } from "./types"

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Expected a model manifest object.")
  return value as Record<string, unknown>
}
function text(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 1024)
    throw new Error("Model fields require bounded nonempty strings.")
  return value
}
function count(value: unknown): string {
  const result = text(value)
  if (!decimal(result) || result === "0")
    throw new Error("Model counts require positive canonical decimal strings.")
  return result
}
function integer(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value <= 0 ||
    value > 384000
  )
    throw new Error("Invalid model audio shape.")
  return value
}
/** Parsing retains only bounded canonical manifest fields, never JSON extras. */
export function parseManifest(json: string): AnalysisModel {
  if (json.length > 16384) throw new Error("Model manifest limit is 16 KiB.")
  const m = record(JSON.parse(json) as unknown)
  const p = record(m.provenance)
  const sha256 = text(m.sha256)
  if (!/^[0-9a-f]{64}$/.test(sha256))
    throw new Error("Expected an explicit lowercase SHA256.")
  if (
    !Array.isArray(m.outputs) ||
    m.outputs.length === 0 ||
    m.outputs.length > 16
  )
    throw new Error("Declare one to sixteen output roles.")
  return {
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
}

function tensors(value: unknown): AnalysisTensorSpec[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > 16)
    throw new Error("Declare one to sixteen named ONNX tensors.")
  return value.map((item: unknown) => {
    const t = record(item)
    if (!Array.isArray(t.shape) || t.shape.length < 1 || t.shape.length > 8)
      throw new Error("ONNX tensors need one to eight explicit dimensions.")
    return { name: text(t.name), shape: t.shape.map((d: unknown) => {
      if (typeof d === "number" && Number.isInteger(d) && d > 0 && d <= 16777216) return d
      if (typeof d === "string" && /^[A-Za-z0-9_]{1,64}$/.test(d)) return d
      throw new Error("Use positive dimensions or bounded symbolic names.")
    }) }
  })
}
function onnx(value: unknown): AnalysisOnnxContract {
  const o = record(value)
  if (!Array.isArray(o.stemRoles) || o.stemRoles.length < 1 || o.stemRoles.length > 16)
    throw new Error("Declare the ONNX stem dimension order.")
  if (typeof o.overlapFrames !== "number" || !Number.isSafeInteger(o.overlapFrames) || o.overlapFrames < 0)
    throw new Error("ONNX overlap must be a nonnegative frame count.")
  if (typeof o.segmentFrames !== "number" || !Number.isSafeInteger(o.segmentFrames) || o.segmentFrames <= o.overlapFrames || o.segmentFrames > 16777216)
    throw new Error("ONNX segment must exceed overlap.")
  return { opset: integer(o.opset), inputs: tensors(o.inputs), outputs: tensors(o.outputs),
    sampleRate: integer(o.sampleRate), segmentFrames: o.segmentFrames,
    overlapFrames: o.overlapFrames, stemRoles: o.stemRoles.map(text) }
}
