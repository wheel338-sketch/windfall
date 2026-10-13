import type { Message } from "../types"

/**
 * The multiband delay's sixteen-band layout: eight banks of sixteen delay,
 * level and pan values. A descriptor name is looked up by its English text,
 * so each of the 384 names is its own key; they follow one pattern, so the
 * entries are built from one sentence per property and language instead of
 * being written out.
 */
const BANKS = [1, 2, 3, 4, 5, 6, 7, 8] as const
const BANDS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16] as const
const PROPERTIES = ["delay", "level", "pan"] as const
/** The headings the generic editor makes from `banks.N.timeMs`, `.level` and `.pan`. */
const HEADINGS = ["time ms", "level", "pan"] as const
const LAYOUTS = [3, 16] as const

type Bank = (typeof BANKS)[number]
type Band = (typeof BANDS)[number]
type Property = (typeof PROPERTIES)[number]
type Heading = (typeof HEADINGS)[number]
type Layout = (typeof LAYOUTS)[number]

type BandDelayKey =
  | `Bank ${Bank} band ${Band} ${Property}`
  | `${Band} ${Property}`
  | `Bank ${Bank} ${Heading}`
  | `${Layout} bands`

interface Wording {
  /** A parameter's full name. */
  name(bank: Bank, band: Band, property: Property): string
  /** The same parameter under its bank's heading. */
  short(band: Band, property: Property): string
  /** The heading of one bank's delays, levels or pans. */
  group(bank: Bank, heading: Heading): string
  layout(bands: Layout): string
}

function entries(wording: Wording): Record<BandDelayKey, string> {
  const result: Partial<Record<BandDelayKey, string>> = {}
  for (const layout of LAYOUTS) result[`${layout} bands`] = wording.layout(layout)
  for (const band of BANDS)
    for (const property of PROPERTIES) result[`${band} ${property}`] = wording.short(band, property)
  for (const bank of BANKS) {
    for (const heading of HEADINGS) result[`Bank ${bank} ${heading}`] = wording.group(bank, heading)
    for (const band of BANDS)
      for (const property of PROPERTIES)
        result[`Bank ${bank} band ${band} ${property}`] = wording.name(bank, band, property)
  }
  return result as Record<BandDelayKey, string>
}

export const descriptorsBandDelayEn = entries({
  name: (bank, band, property) => `Bank ${bank} band ${band} ${property}`,
  short: (band, property) => `${band} ${property}`,
  group: (bank, heading) => `Bank ${bank} ${heading}`,
  layout: (bands) => `${bands} bands`,
})

const PROPERTY_ES: Record<Property, string> = { delay: "Retardo", level: "Nivel", pan: "Panorama" }
const HEADING_ES: Record<Heading, (bank: Bank) => string> = {
  "time ms": (bank) => `Tiempos del banco ${bank} (ms)`,
  level: (bank) => `Niveles del banco ${bank}`,
  pan: (bank) => `Panoramas del banco ${bank}`,
}

export const descriptorsBandDelayEs = entries({
  name: (bank, band, property) => `${PROPERTY_ES[property]} de la banda ${band} del banco ${bank}`,
  short: (band, property) => `${PROPERTY_ES[property]} ${band}`,
  group: (bank, heading) => HEADING_ES[heading](bank),
  layout: (bands) => `${bands} bandas`,
}) satisfies Record<keyof typeof descriptorsBandDelayEn, Message>
