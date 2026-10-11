export function wav(samples: number[], channels = 1): ArrayBuffer {
  const buffer = new ArrayBuffer(44 + samples.length * 4), view = new DataView(buffer)
  const tag = (at: number, value: string) => [...value].forEach((char, i) => view.setUint8(at + i, char.charCodeAt(0)))
  tag(0, "RIFF"); view.setUint32(4, buffer.byteLength - 8, true); tag(8, "WAVE"); tag(12, "fmt ")
  view.setUint32(16, 16, true); view.setUint16(20, 3, true); view.setUint16(22, channels, true); view.setUint32(24, 48000, true)
  view.setUint32(28, 48000 * channels * 4, true); view.setUint16(32, channels * 4, true); view.setUint16(34, 32, true)
  tag(36, "data"); view.setUint32(40, samples.length * 4, true); samples.forEach((sample, i) => view.setFloat32(44 + i * 4, sample, true))
  return buffer
}
