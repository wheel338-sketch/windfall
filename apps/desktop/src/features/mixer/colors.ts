import { translatedText as t } from "@/lib/i18n"

/**
 * The colors a track can be given: the palette new tracks cycle through
 * (`PALETTE` in `windfall-project`) and the grey of the master.
 */
export const TRACK_COLORS: readonly { color: number; name: string }[] = [
  { color: 0xe5484d, get name() { return t("Red") } },
  { color: 0xf76b15, get name() { return t("Orange") } },
  { color: 0xffb224, get name() { return t("Amber") } },
  { color: 0x99d52a, get name() { return t("Lime") } },
  { color: 0x46a758, get name() { return t("Green") } },
  { color: 0x12a594, get name() { return t("Teal") } },
  { color: 0x05a2c2, get name() { return t("Cyan") } },
  { color: 0x3e63dd, get name() { return t("Blue") } },
  { color: 0x6e56cf, get name() { return t("Violet") } },
  { color: 0x8e4ec6, get name() { return t("Purple") } },
  { color: 0xd6409f, get name() { return t("Pink") } },
  { color: 0xe93d82, get name() { return t("Rose") } },
  { color: 0x8b8d98, get name() { return t("Grey") } },
]
