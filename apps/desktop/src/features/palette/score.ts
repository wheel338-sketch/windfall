/**
 * Letters scattered across a whole title match almost anything, so a loose
 * match must have its letters at least this close together.
 */
const MIN_TIGHTNESS = 0.3

/** The bands a match falls in, best first. A band never reaches the one above. */
const EXACT = 1
const TITLE_START = 0.9
const WORD_STARTS_IN_ORDER = 0.8
const WORD_STARTS = 0.7
const INSIDE_TITLE = 0.6
const WITH_KEYWORDS = 0.45
const LOOSE = 0.2
/** How far a better fit can lift a match inside its band. */
const BAND = 0.09

const words = (text: string) => text.split(/\s+/).filter((word) => word !== "")

/** A title as it is compared: lower case, without the "…" of a dialog. */
const folded = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase()
const plain = (title: string) => folded(title).replace(/…$/, "").trim()

/**
 * Whether each word of the query starts a word of the title, the words in
 * the order they were typed and each title word used once.
 */
function startsInOrder(
  titleWords: readonly string[],
  queryWords: readonly string[]
): boolean {
  let from = 0
  for (const word of queryWords) {
    const at = titleWords.findIndex(
      (candidate, index) => index >= from && candidate.startsWith(word)
    )
    if (at < 0) return false
    from = at + 1
  }
  return true
}

/**
 * How well a search matches an action, from 0 (hide it) to 1.
 *
 * - The search is the title: 1.
 * - The title starts with it.
 * - Its words start words of the title, in the order typed, then in any.
 * - It is inside the title somewhere.
 * - Its words start words of the title or of the keywords. A keyword is
 *   worth less than a word of the title, whatever it matches.
 * - Its letters only appear in the title in order ("sv as" for "Save as").
 *   This is the last resort and never beats a match on whole words:
 *   "tall" is in "Select all" that way, and belongs under "Tall tracks".
 *
 * Inside a band, a title the search covers more of scores a little
 * higher, so "play" puts "Play" before "Play the pattern".
 */
export function scoreAction(
  title: string,
  keywords: string,
  search: string
): number {
  const query = folded(search).trim().replace(/\s+/g, " ")
  if (query === "") return 1

  const name = plain(title)
  if (name === query) return EXACT
  // How much of the title the search accounts for, 0 to 1.
  const cover = Math.min(1, query.replace(/ /g, "").length / name.length)
  const within = (band: number) => band + BAND * cover

  if (name.startsWith(query)) return within(TITLE_START)

  const nameWords = words(name)
  const queryWords = words(query)
  if (startsInOrder(nameWords, queryWords)) return within(WORD_STARTS_IN_ORDER)
  const startsWords = queryWords.every((word) =>
    nameWords.some((candidate) => candidate.startsWith(word))
  )
  if (startsWords) return within(WORD_STARTS)
  if (name.includes(query)) return within(INSIDE_TITLE)

  const extraWords = words(folded(keywords))
  const startsAny = (word: string, among: readonly string[]) =>
    among.some((candidate) => candidate.startsWith(word))
  if (
    queryWords.every(
      (word) => startsAny(word, nameWords) || startsAny(word, extraWords)
    )
  ) {
    // More of the search found in the title itself is the better match.
    const inTitle = queryWords.filter((word) => startsAny(word, nameWords))
    return WITH_KEYWORDS + BAND * (inTitle.length / queryWords.length)
  }

  const loose = inOrder(name, query.replace(/ /g, ""))
  return loose < MIN_TIGHTNESS ? 0 : LOOSE + 0.2 * loose
}

/**
 * The share of a search's words, 0 to 1, that start a word of the title or
 * the keywords. For a search nothing matches in full, so the palette can
 * still point at the nearest actions. Words under three letters are left
 * out: they start too many.
 */
export function shareOfWords(
  title: string,
  keywords: string,
  search: string
): number {
  const queryWords = words(folded(search)).filter((word) => word.length >= 3)
  if (queryWords.length === 0) return 0
  const among = [...words(plain(title)), ...words(folded(keywords))]
  const found = queryWords.filter((word) =>
    among.some((candidate) => candidate.startsWith(word))
  )
  return found.length / queryWords.length
}

/**
 * Whether the letters of `query` appear in `text` in order. Returns 0 when
 * they do not, and otherwise how tightly they sit together, up to 1.
 */
function inOrder(text: string, query: string): number {
  let from = text.indexOf(query[0])
  if (from < 0) return 0
  const first = from
  for (const letter of query.slice(1)) {
    from = text.indexOf(letter, from + 1)
    if (from < 0) return 0
  }
  return query.length / (from - first + 1)
}

/**
 * The things that match a search, best first. Equal matches keep the order
 * they came in, which is the order of the menus.
 */
export function rank<T>(
  items: readonly T[],
  search: string,
  describe: (item: T) => { title: string; keywords: string }
): T[] {
  return items
    .map((item, index) => {
      const { title, keywords } = describe(item)
      return { item, index, score: scoreAction(title, keywords, search) }
    })
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.item)
}
