// The module is checked in beside this file. `?url&inline` makes Vite hand its
// bytes over as a base64 data URL inside the script: `?url` because Vite does
// not count a `.wasm` file among the assets it imports by default, `inline`
// so that the dev server, the production build and the tests all load it the
// same way, with nothing to fetch.
import moduleUrl from "./windfall_sim.wasm?url&inline"
import { message } from "@/lib/i18n"

/**
 * The Rust `Document` (crate `windfall-sim`), compiled to WebAssembly. Every
 * operation takes a document handle and JSON, and answers with JSON.
 */
export type Sim = {
  /**
   * Runs an operation by its exported name. Throws an `Error` that carries
   * the operation's own message when it fails.
   */
  call<T>(operation: string, handle?: number, input?: unknown): T
  /** How much memory the module holds. WebAssembly memory never shrinks. */
  memoryBytes(): number
}

type Operation = (handle: number, input: number, length: number) => number

type Reply<T> = { ok: T } | { error: string }

function moduleBytes() {
  const binary = atob(moduleUrl.slice(moduleUrl.indexOf(",") + 1))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

/** Starts a module of its own, sharing no documents with any other. */
export async function instantiateSim(): Promise<Sim> {
  const { instance } = await WebAssembly.instantiate(moduleBytes())
  const exports = instance.exports
  const memory = exports.memory as WebAssembly.Memory
  const alloc = exports.sim_alloc as (length: number) => number
  const dealloc = exports.sim_dealloc as (
    buffer: number,
    length: number
  ) => void
  const panicMessage = exports.sim_panic_message as () => number
  const encoder = new TextEncoder()
  const decoder = new TextDecoder()
  let crashed: string | null = null

  /** Reads a result, four bytes of length and then text, and frees it. */
  function take(result: number): string {
    // Looked up on every call: the buffer is replaced when memory grows.
    const length = new DataView(memory.buffer).getUint32(result, true)
    const text = decoder.decode(
      new Uint8Array(memory.buffer, result + 4, length)
    )
    dealloc(result, length + 4)
    return text
  }

  /**
   * A Rust panic stops the module in the middle of whatever it was doing,
   * so nothing it holds can be trusted again. Every later call says so
   * instead of working on with documents that may be half edited.
   */
  function crash(trap: unknown): Error {
    let said = ""
    try {
      said = take(panicMessage())
    } catch {
      // The trap below is all there is to report.
    }
    crashed = message("The WebAssembly document crashed, and stays unusable until the page is reloaded: {reason}", { reason: said || String(trap) })
    return new Error(crashed)
  }

  function call<T>(operation: string, handle = 0, input?: unknown): T {
    if (crashed !== null) throw new Error(crashed)
    const run = exports[operation]
    if (typeof run !== "function") {
      throw new Error(
        message("The WebAssembly document has no operation \"{operation}\". Run scripts/build-sim.sh.", { operation })
      )
    }
    const bytes =
      input === undefined ? null : encoder.encode(JSON.stringify(input))
    const length = bytes?.length ?? 0
    let text: string
    try {
      const buffer = bytes ? alloc(length) : 0
      if (bytes) new Uint8Array(memory.buffer, buffer, length).set(bytes)
      const result = (run as Operation)(handle, buffer, length)
      if (bytes) dealloc(buffer, length)
      text = take(result)
    } catch (trap) {
      throw crash(trap)
    }
    const reply = JSON.parse(text) as Reply<T>
    if ("error" in reply) throw new Error(reply.error)
    return reply.ok
  }

  return { call, memoryBytes: () => memory.buffer.byteLength }
}

/**
 * The module the mock backend uses. It is ready before anything that
 * imports this file runs, so creating a mock never has to wait.
 */
export const sim: Sim = await instantiateSim()
