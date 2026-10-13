import type { RectBatch } from "./rect-batch"
import {
  BORDER_ROUNDING,
  BORDER_SHADE,
  RendererUnavailableError,
  type DrawOptions,
  type RectRenderer,
  type RendererInfo,
} from "./renderer"
import type { GridTheme } from "./theme"
import type { DeviceTransform } from "./viewport"
import { message, plainNumber } from "@/lib/i18n"

// The same instanced quad as the WebGL2 renderer, line for line, so both
// put a rect on the same pixels.
const SHADER = /* wgsl */ `
struct Uniforms {
  resolution: vec2f,
  lineWidth: f32,
  drawPass: i32,
  transform: vec4f,
  ticks: vec4i,
  selectionFill: vec4f,
  selectionBorder: vec4f,
  resize: vec4i,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexOut {
  @builtin(position) position: vec4f,
  @location(0) @interpolate(flat) fill: vec4f,
  @location(1) @interpolate(flat) border: vec4f,
  @location(2) @interpolate(flat) size: vec2f,
  @location(3) @interpolate(flat) borderWidth: f32,
  @location(4) local: vec2f,
}

fn snap(v: f32) -> f32 {
  return floor(v + 0.5);
}

@vertex
fn vertexMain(
  @builtin(vertex_index) vertexIndex: u32,
  @location(0) geometry: vec4i,
  @location(1) color: vec4f,
  @location(2) flags: u32,
) -> VertexOut {
  var result: VertexOut;
  let selected = (flags & 1u) != 0u;
  if ((u.drawPass == 1 && selected) || (u.drawPass == 2 && !selected)) {
    result.position = vec4f(2.0, 2.0, 0.0, 1.0);
    return result;
  }
  let isFlat = (flags & 2u) != 0u;
  var start = geometry.x - u.ticks.x;
  var len = geometry.y;
  var row = geometry.z;
  if (selected) {
    // resizedSpan in renderer.ts.
    let keep = min(len, u.resize.z);
    let shift = min(u.resize.x, len - keep);
    len -= shift;
    len = max(len + u.resize.y, min(len, u.resize.z));
    start += shift + u.ticks.y;
    row += u.ticks.z;
  }
  let lw = u.lineWidth;
  var x0 = snap(f32(start) * u.transform.x - u.transform.y);
  var x1 = snap(f32(start + len) * u.transform.x - u.transform.y);
  var y0 = snap(f32(row) * u.transform.z - u.transform.w);
  var y1 = snap(f32(row + geometry.w) * u.transform.z - u.transform.w);
  if (!isFlat) { y0 += lw; }
  if ((flags & 4u) != 0u) { x0 = 0.0; x1 = u.resolution.x; }
  if ((flags & 8u) != 0u) { y0 = 0.0; y1 = u.resolution.y; }
  if ((flags & 16u) != 0u) { x1 = x0 + lw; }
  if ((flags & 32u) != 0u) { y1 = y0 + lw; }
  x1 = max(x1, x0 + lw);
  y1 = max(y1, y0 + lw);

  let size = vec2f(x1 - x0, y1 - y0);
  let corner = vec2f(f32(vertexIndex & 1u), f32(vertexIndex >> 1u));
  let position = vec2f(x0, y0) + corner * size;

  var fill = color.rgb;
  // shadeChannel in renderer.ts.
  var border = floor(fill * 255.0 * ${BORDER_SHADE} + ${BORDER_ROUNDING}) / 255.0;
  if (selected) {
    fill = mix(fill, u.selectionFill.rgb, u.selectionFill.a);
    border = u.selectionBorder.rgb;
  }
  result.fill = vec4f(fill * color.a, color.a);
  result.border = vec4f(border * color.a, color.a);
  result.size = size;
  result.borderWidth = 0.0;
  if (!isFlat && size.x >= 3.0 * lw && size.y >= 3.0 * lw) {
    result.borderWidth = lw;
  }
  result.local = corner * size;
  result.position = vec4f(
    position / u.resolution * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
  return result;
}

@fragment
fn fragmentMain(input: VertexOut) -> @location(0) vec4f {
  let edge = min(input.local, input.size - input.local);
  if (min(edge.x, edge.y) < input.borderWidth) {
    return input.border;
  }
  return input.fill;
}
`

// TypeScript's DOM library declares the WebGPU interfaces but not these
// flag constants. The values are fixed by the WebGPU specification.
const BUFFER_MAP_READ = 0x0001
const BUFFER_COPY_SRC = 0x0004
const BUFFER_COPY_DST = 0x0008
const BUFFER_VERTEX = 0x0020
const BUFFER_UNIFORM = 0x0040
const BUFFER_QUERY_RESOLVE = 0x0200
const SHADER_STAGE_VERTEX = 0x1
const MAP_MODE_READ = 0x1

const UNIFORM_BYTES = 96
// Dynamic uniform offsets must be multiples of 256 bytes.
const SLOT_BYTES = 256
const MAX_SLOTS = 128
const NO_RESIZE: DrawOptions = {}

const PASS_ALL = 0
const PASS_UNSELECTED = 1
const PASS_SELECTED = 2

interface GpuBatch {
  geometry: GPUBuffer
  colors: GPUBuffer
  flags: GPUBuffer
  capacity: number
  geometryVersion: number
  colorVersion: number
  flagsVersion: number
}

interface Frame {
  encoder: GPUCommandEncoder
  pass: GPURenderPassEncoder
  transform: DeviceTransform
  slot: number
  timed: boolean
}

class WebGPURenderer implements RectRenderer {
  readonly info: RendererInfo
  onRestored: (() => void) | null = null
  onLost: (() => void) | null = null

  private readonly canvas: HTMLCanvasElement
  private readonly device: GPUDevice
  private readonly context: GPUCanvasContext
  private readonly pipeline: GPURenderPipeline
  private readonly uniformBuffer: GPUBuffer
  private readonly bindGroup: GPUBindGroup
  private readonly staging = new ArrayBuffer(SLOT_BYTES * MAX_SLOTS)
  private readonly stagingFloats = new Float32Array(this.staging)
  private readonly stagingInts = new Int32Array(this.staging)
  private readonly querySet: GPUQuerySet | null
  private readonly resolveBuffer: GPUBuffer | null
  private readonly readBuffer: GPUBuffer | null
  private batches = new Map<RectBatch, GpuBatch>()
  private theme: GridTheme | null = null
  private frame: Frame | null = null
  private timing = false
  private readPending = false
  private gpuTimes: number[] = []
  private lost = false

  constructor(
    canvas: HTMLCanvasElement,
    context: GPUCanvasContext,
    device: GPUDevice,
    deviceName: string
  ) {
    this.canvas = canvas
    this.context = context
    this.device = device
    const format = navigator.gpu.getPreferredCanvasFormat()
    context.configure({ device, format, alphaMode: "opaque" })

    const module = device.createShaderModule({ code: SHADER })
    const bindGroupLayout = device.createBindGroupLayout({
      entries: [
        {
          binding: 0,
          visibility: SHADER_STAGE_VERTEX,
          buffer: {
            type: "uniform",
            hasDynamicOffset: true,
            minBindingSize: UNIFORM_BYTES,
          },
        },
      ],
    })
    const premultiplied: GPUBlendComponent = {
      srcFactor: "one",
      dstFactor: "one-minus-src-alpha",
      operation: "add",
    }
    this.pipeline = device.createRenderPipeline({
      layout: device.createPipelineLayout({
        bindGroupLayouts: [bindGroupLayout],
      }),
      vertex: {
        module,
        entryPoint: "vertexMain",
        buffers: [
          {
            arrayStride: 16,
            stepMode: "instance",
            attributes: [{ shaderLocation: 0, offset: 0, format: "sint32x4" }],
          },
          {
            arrayStride: 4,
            stepMode: "instance",
            attributes: [{ shaderLocation: 1, offset: 0, format: "unorm8x4" }],
          },
          {
            arrayStride: 4,
            stepMode: "instance",
            attributes: [{ shaderLocation: 2, offset: 0, format: "uint32" }],
          },
        ],
      },
      fragment: {
        module,
        entryPoint: "fragmentMain",
        targets: [
          { format, blend: { color: premultiplied, alpha: premultiplied } },
        ],
      },
      primitive: { topology: "triangle-strip" },
    })
    this.uniformBuffer = device.createBuffer({
      size: SLOT_BYTES * MAX_SLOTS,
      usage: BUFFER_UNIFORM | BUFFER_COPY_DST,
    })
    this.bindGroup = device.createBindGroup({
      layout: bindGroupLayout,
      entries: [
        {
          binding: 0,
          resource: { buffer: this.uniformBuffer, size: UNIFORM_BYTES },
        },
      ],
    })

    const timestamps = device.features.has("timestamp-query")
    this.querySet = timestamps
      ? device.createQuerySet({ type: "timestamp", count: 2 })
      : null
    this.resolveBuffer = timestamps
      ? device.createBuffer({
          size: 16,
          usage: BUFFER_QUERY_RESOLVE | BUFFER_COPY_SRC,
        })
      : null
    this.readBuffer = timestamps
      ? device.createBuffer({
          size: 16,
          usage: BUFFER_MAP_READ | BUFFER_COPY_DST,
        })
      : null
    this.info = {
      kind: "webgpu",
      device: deviceName,
      gpuTiming: timestamps ? "timestamp-query" : "none",
    }
    void device.lost.then((lost) => {
      this.lost = true
      // A device this renderer destroyed itself is not a loss to report.
      if (lost.reason !== "destroyed") this.onLost?.()
    })
  }

  resize(widthDev: number, heightDev: number): void {
    if (this.canvas.width !== widthDev) this.canvas.width = widthDev
    if (this.canvas.height !== heightDev) this.canvas.height = heightDev
  }

  setTheme(theme: GridTheme): void {
    this.theme = theme
  }

  beginFrame(transform: DeviceTransform): void {
    if (this.lost) return
    const background = this.theme?.background ?? { r: 0, g: 0, b: 0 }
    const timed = this.timing && this.querySet !== null && !this.readPending
    const encoder = this.device.createCommandEncoder()
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view: this.context.getCurrentTexture().createView(),
          clearValue: {
            r: background.r / 255,
            g: background.g / 255,
            b: background.b / 255,
            a: 1,
          },
          loadOp: "clear",
          storeOp: "store",
        },
      ],
      ...(timed && this.querySet
        ? {
            timestampWrites: {
              querySet: this.querySet,
              beginningOfPassWriteIndex: 0,
              endOfPassWriteIndex: 1,
            },
          }
        : {}),
    })
    pass.setPipeline(this.pipeline)
    this.frame = { encoder, pass, transform, slot: 0, timed }
  }

  drawBatch(batch: RectBatch, options: DrawOptions = {}): void {
    const frame = this.frame
    if (!frame) return
    const first = Math.max(0, options.first ?? 0)
    const last = Math.min(batch.count, options.last ?? batch.count)
    if (last <= first) return

    const gpu = this.upload(batch)
    const pass = frame.pass
    pass.setVertexBuffer(0, gpu.geometry)
    pass.setVertexBuffer(1, gpu.colors)
    pass.setVertexBuffer(2, gpu.flags)
    const instances = last - first
    if (batch.selectedCount === 0) {
      this.bindUniforms(frame, PASS_ALL, 0, 0, NO_RESIZE)
      pass.draw(4, instances, 0, first)
      return
    }
    const dragTicks = options.dragTicks ?? 0
    const dragRows = options.dragRows ?? 0
    this.bindUniforms(frame, PASS_UNSELECTED, dragTicks, dragRows, options)
    pass.draw(4, instances, 0, first)
    this.bindUniforms(frame, PASS_SELECTED, dragTicks, dragRows, options)
    pass.draw(4, instances, 0, first)
  }

  endFrame(): void {
    const frame = this.frame
    if (!frame) return
    this.frame = null
    frame.pass.end()
    const { querySet, resolveBuffer, readBuffer } = this
    const timed = frame.timed && querySet && resolveBuffer && readBuffer
    if (timed) {
      frame.encoder.resolveQuerySet(querySet, 0, 2, resolveBuffer, 0)
      frame.encoder.copyBufferToBuffer(resolveBuffer, 0, readBuffer, 0, 16)
    }
    this.device.queue.submit([frame.encoder.finish()])
    if (timed) {
      // The read buffer cannot be copied into while it is mapped, so the
      // next frames go untimed until this one has been read.
      this.readPending = true
      void readBuffer.mapAsync(MAP_MODE_READ).then(
        () => {
          const stamps = new BigInt64Array(readBuffer.getMappedRange())
          this.gpuTimes.push(Number(stamps[1] - stamps[0]) / 1e6)
          readBuffer.unmap()
          this.readPending = false
        },
        () => {
          this.readPending = false
        }
      )
    }
  }

  release(batch: RectBatch): void {
    const gpu = this.batches.get(batch)
    if (!gpu) return
    gpu.geometry.destroy()
    gpu.colors.destroy()
    gpu.flags.destroy()
    this.batches.delete(batch)
  }

  // WebGPU can only report finished work asynchronously.
  finish(): boolean {
    return false
  }

  setGpuTiming(enabled: boolean): boolean {
    this.timing = enabled && this.querySet !== null
    return this.timing
  }

  takeGpuTimes(): number[] {
    const times = this.gpuTimes
    this.gpuTimes = []
    return times
  }

  dispose(): void {
    for (const batch of [...this.batches.keys()]) this.release(batch)
    this.uniformBuffer.destroy()
    this.querySet?.destroy()
    this.resolveBuffer?.destroy()
    this.readBuffer?.destroy()
    this.context.unconfigure()
    this.device.destroy()
  }

  private bindUniforms(
    frame: Frame,
    drawPass: number,
    dragTicks: number,
    dragRows: number,
    resize: DrawOptions
  ): void {
    if (frame.slot >= MAX_SLOTS) {
      throw new Error(message("more than {maxSlots} draws in one frame", { maxSlots: plainNumber(MAX_SLOTS) }))
    }
    const t = frame.transform
    const base = (frame.slot * SLOT_BYTES) / 4
    const floats = this.stagingFloats
    const ints = this.stagingInts
    floats[base] = this.canvas.width
    floats[base + 1] = this.canvas.height
    floats[base + 2] = t.lineWidth
    ints[base + 3] = drawPass
    floats[base + 4] = t.scaleX
    floats[base + 5] = t.offsetX
    floats[base + 6] = t.scaleY
    floats[base + 7] = t.offsetY
    ints[base + 8] = t.scrollTick
    ints[base + 9] = dragTicks
    ints[base + 10] = dragRows
    ints[base + 11] = 0
    ints[base + 20] = resize.resizeStart ?? 0
    ints[base + 21] = resize.resizeEnd ?? 0
    ints[base + 22] = resize.minLength ?? 0
    ints[base + 23] = 0
    const theme = this.theme
    if (theme) {
      floats[base + 12] = theme.selectionFill.r / 255
      floats[base + 13] = theme.selectionFill.g / 255
      floats[base + 14] = theme.selectionFill.b / 255
      floats[base + 15] = theme.selectionMix
      floats[base + 16] = theme.selectionBorder.r / 255
      floats[base + 17] = theme.selectionBorder.g / 255
      floats[base + 18] = theme.selectionBorder.b / 255
      floats[base + 19] = 1
    }
    const offset = frame.slot * SLOT_BYTES
    this.device.queue.writeBuffer(
      this.uniformBuffer,
      offset,
      this.staging,
      offset,
      UNIFORM_BYTES
    )
    frame.pass.setBindGroup(0, this.bindGroup, [offset])
    frame.slot++
  }

  private upload(batch: RectBatch): GpuBatch {
    const device = this.device
    let gpu = this.batches.get(batch)
    const count = batch.count
    if (!gpu || count > gpu.capacity) {
      if (gpu) this.release(batch)
      const capacity = Math.max(count, (gpu?.capacity ?? 0) * 2, 64)
      const usage = BUFFER_VERTEX | BUFFER_COPY_DST
      gpu = {
        geometry: device.createBuffer({ size: capacity * 16, usage }),
        colors: device.createBuffer({ size: capacity * 4, usage }),
        flags: device.createBuffer({ size: capacity * 4, usage }),
        capacity,
        geometryVersion: -1,
        colorVersion: -1,
        flagsVersion: -1,
      }
      this.batches.set(batch, gpu)
    }
    if (gpu.geometryVersion !== batch.geometryVersion) {
      device.queue.writeBuffer(gpu.geometry, 0, batch.geometry, 0, count * 4)
      gpu.geometryVersion = batch.geometryVersion
    }
    if (gpu.colorVersion !== batch.colorVersion) {
      device.queue.writeBuffer(gpu.colors, 0, batch.colors, 0, count * 4)
      gpu.colorVersion = batch.colorVersion
    }
    if (gpu.flagsVersion !== batch.flagsVersion) {
      device.queue.writeBuffer(gpu.flags, 0, batch.flags, 0, count)
      gpu.flagsVersion = batch.flagsVersion
    }
    return gpu
  }
}

export async function createWebGPURenderer(
  canvas: HTMLCanvasElement
): Promise<RectRenderer> {
  if (!("gpu" in navigator)) {
    throw new RendererUnavailableError("webgpu", "navigator.gpu is missing")
  }
  const adapter = await navigator.gpu.requestAdapter({
    powerPreference: "high-performance",
  })
  if (!adapter) throw new RendererUnavailableError("webgpu", "no adapter")
  const device = await adapter.requestDevice({
    requiredFeatures: adapter.features.has("timestamp-query")
      ? ["timestamp-query"]
      : [],
  })
  const context = canvas.getContext("webgpu")
  if (!(context instanceof GPUCanvasContext)) {
    device.destroy()
    throw new RendererUnavailableError("webgpu", "no webgpu canvas context")
  }
  const { vendor, architecture, device: model, description } = adapter.info
  const deviceName = [vendor, architecture, model, description]
    .filter((part) => part !== "")
    .join(" ")
  return new WebGPURenderer(canvas, context, device, deviceName || "unknown")
}
