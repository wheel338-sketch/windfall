export const MAX_RIFF_SEED = 0xffffffff

export type RiffRng = {
  /** Uniform in [0, 1). */
  next(): number
  /**
   * A stream named under this one. It depends only on the seed and the path
   * of names, never on how much any other stream has been used.
   */
  fork(name: string): RiffRng
}

function streamState(seed: number, path: string): number {
  // FNV-1a over the path, started from the seed, then a murmur3 finaliser.
  let hash = (2166136261 ^ seed) >>> 0
  for (let index = 0; index < path.length; index++) {
    hash ^= path.charCodeAt(index)
    hash = Math.imul(hash, 16777619) >>> 0
  }
  hash ^= hash >>> 16
  hash = Math.imul(hash, 0x85ebca6b) >>> 0
  hash ^= hash >>> 13
  hash = Math.imul(hash, 0xc2b2ae35) >>> 0
  return (hash ^ (hash >>> 16)) >>> 0
}

/** Mulberry32 streams addressed by name, so stages never share draws. */
export function createRiffRng(seed: number, path = ""): RiffRng {
  let state = streamState(seed >>> 0, path)
  return {
    next() {
      state = (state + 0x6d2b79f5) >>> 0
      let mixed = state
      mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1)
      mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61)
      return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296
    },
    fork: (name) => createRiffRng(seed, `${path}/${name}`),
  }
}
