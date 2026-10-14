/** English source and Spanish copy of the native common codes. */
export const nativeCommonEn = {
  "text.term": "{text}",
  "text.sentence": "{text}.",
} as const

export const nativeCommonEs = {
  "text.term": "{text}",
  "text.sentence": "{text}.",
} satisfies Record<keyof typeof nativeCommonEn, string | { one: string; other: string }>
