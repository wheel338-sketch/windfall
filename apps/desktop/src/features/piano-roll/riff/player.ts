export type RiffPlayerSink = {
  on(key: number, velocity: number): void
  off(key: number): void
  /** Called once, when playback ends or is stopped. */
  ended(): void
}

type Playback = {
  timers: ReturnType<typeof setTimeout>[]
  /** Key to the note that is sounding on it. */
  held: Map<number, number>
  sink: RiffPlayerSink
}

/**
 * Plays notes with timers. Whatever ends playback (its last note, `stop`, or
 * a new `start`) releases every key that is still held, exactly once.
 */
export function createRiffPlayer() {
  let playback: Playback | null = null

  function stop() {
    const current = playback
    if (!current) return
    playback = null
    for (const timer of current.timers) clearTimeout(timer)
    for (const key of current.held.keys()) current.sink.off(key)
    current.held.clear()
    current.sink.ended()
  }

  function start(
    notes: readonly {
      start: number
      length: number
      key: number
      velocity: number
    }[],
    msPerTick: number,
    sink: RiffPlayerSink
  ) {
    stop()
    if (notes.length === 0) {
      sink.ended()
      return
    }
    const current: Playback = { timers: [], held: new Map(), sink }
    playback = current
    const first = Math.min(...notes.map((note) => note.start))
    const last = Math.max(...notes.map((note) => note.start + note.length))
    const at = (tick: number) => Math.round((tick - first) * msPerTick)
    notes.forEach((note, index) => {
      current.timers.push(
        setTimeout(() => {
          // The engine holds one voice per key: retrigger a held one.
          if (current.held.has(note.key)) sink.off(note.key)
          current.held.set(note.key, index)
          sink.on(note.key, note.velocity)
        }, at(note.start)),
        setTimeout(
          () => {
            if (current.held.get(note.key) !== index) return
            current.held.delete(note.key)
            sink.off(note.key)
          },
          at(note.start + note.length)
        )
      )
    })
    current.timers.push(setTimeout(stop, at(last)))
  }

  return {
    start,
    stop,
    get playing() {
      return playback !== null
    },
  }
}
