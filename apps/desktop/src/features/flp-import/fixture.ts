/** Original synthetic fixture dedicated to CC0-1.0; no Image-Line bytes. */
export function flpFixture() {
  const events: number[] = []
  const word = (id: number, value: number) =>
    events.push(id, value & 255, value >> 8)
  const data = (id: number, bytes: number[]) => {
    events.push(id, bytes.length, ...bytes)
  }
  const text = (id: number, value: string) =>
    data(
      id,
      [...value, "\0"].flatMap((c) => [c.charCodeAt(0), 0])
    )
  data(199, [...new TextEncoder().encode("20.8.4.2576"), 0])
  word(64, 0)
  events.push(21, 2)
  text(201, "Unknown synth")
  data(213, [0, 255, 17, 90])
  word(64, 1)
  events.push(21, 0)
  text(196, "missing.wav")
  word(65, 1)
  const note = new Uint8Array(24)
  const view = new DataView(note.buffer)
  view.setUint32(8, 96, true)
  view.setUint16(12, 60, true)
  note[16] = 120
  note[20] = 64
  note[21] = 100
  data(224, Array.from(note))
  const bytes = new Uint8Array(22 + events.length)
  const header = new DataView(bytes.buffer)
  bytes.set(new TextEncoder().encode("FLhd"))
  header.setUint32(4, 6, true)
  header.setUint16(10, 2, true)
  header.setUint16(12, 96, true)
  bytes.set(new TextEncoder().encode("FLdt"), 14)
  header.setUint32(18, events.length, true)
  bytes.set(events, 22)
  return bytes
}

/**
 * Original synthetic fixture dedicated to CC0-1.0; no Image-Line bytes. A
 * SimSynth channel and a Fruity Soft Clipper on the master, saved as FL
 * Studio 9 writes them: both have decoded fields their Windfall stand-ins
 * have no control for (the synth's warmth, track-amp and chorus switches,
 * the clipper's post gain).
 */
export function flpOmittedFixture() {
  const events: number[] = []
  const byte = (id: number, value: number) => events.push(id, value & 255)
  const word = (id: number, value: number) =>
    events.push(id, value & 255, (value >> 8) & 255)
  const dword = (id: number, value: number) =>
    events.push(
      id,
      value & 255,
      (value >>> 8) & 255,
      (value >>> 16) & 255,
      (value >>> 24) & 255
    )
  const data = (id: number, bytes: number[]) => {
    events.push(id)
    let length = bytes.length
    for (;;) {
      const low = length & 0x7f
      length >>= 7
      if (length === 0) {
        events.push(low)
        break
      }
      events.push(low | 0x80)
    }
    events.push(...bytes)
  }
  const text = (id: number, value: string) =>
    data(id, [...new TextEncoder().encode(value), 0])
  text(199, "9.0.0")
  word(66, 120)
  word(64, 0)
  byte(21, 2)
  text(201, "SimSynth")
  // 45 doubles, 76 reserved bytes and twenty flag words.
  data(213, Array.from({ length: 516 }, () => 0))
  text(204, "Master")
  text(201, "Fruity Soft Clipper")
  // threshold 100, postgain 64.
  data(213, [100, 0, 0, 0, 64, 0, 0, 0])
  word(98, 0)
  dword(147, 0xffffffff)
  const bytes = new Uint8Array(22 + events.length)
  const header = new DataView(bytes.buffer)
  bytes.set(new TextEncoder().encode("FLhd"))
  header.setUint32(4, 6, true)
  header.setUint16(10, 1, true)
  header.setUint16(12, 96, true)
  bytes.set(new TextEncoder().encode("FLdt"), 14)
  header.setUint32(18, events.length, true)
  bytes.set(events, 22)
  return bytes
}
