import { describe, expect, it } from "vitest"

import { CHORDS, detectChord, PITCH_NAMES, pitchClass } from "./detection"

const named = (keys: number[]) => {
  const { symbol, label } = detectChord(keys)
  return [symbol, label]
}

describe("notes and intervals", () => {
  it("names a single pitch class by its note, in any octave", () => {
    expect(detectChord([60])).toMatchObject({
      kind: "note",
      label: "C",
      symbol: "C",
      root: 0,
      bass: 0,
      quality: null,
      chord: null,
      pitchClasses: [0],
      intervals: [0],
      degrees: ["1"],
    })
    expect(named([36, 48, 60, 72])).toEqual(["C", "C"])
    expect(named([70])).toEqual(["A♯", "A♯"])
  })

  it.each([
    [1, "C–C♯", "C–C♯ minor second", "♭2"],
    [2, "C–D", "C–D major second", "2"],
    [3, "C–D♯", "C–D♯ minor third", "♭3"],
    [4, "C–E", "C–E major third", "3"],
    [5, "C–F", "C–F perfect fourth", "4"],
    [6, "C–F♯", "C–F♯ tritone", "♭5"],
    [7, "C5", "C power chord", "5"],
    [8, "C–G♯", "C–G♯ minor sixth", "♭6"],
    [9, "C–A", "C–A major sixth", "6"],
    [10, "C–A♯", "C–A♯ minor seventh", "♭7"],
    [11, "C–B", "C–B major seventh", "7"],
  ])("names %i semitones above the bass", (semitones, symbol, label, degree) => {
    expect(detectChord([60, 60 + semitones])).toMatchObject({
      kind: "interval",
      symbol,
      label,
      root: 0,
      bass: 0,
      quality: null,
      chord: semitones === 7 ? "5" : null,
      intervals: [0, semitones],
      degrees: ["1", degree],
    })
    // The octave of the upper note and doublings change nothing.
    expect(named([48, 60, 72 + semitones, 84 + semitones])).toEqual([
      symbol,
      label,
    ])
  })

  it("measures an interval up from the lowest key, so a fifth turned over is a fourth", () => {
    expect(named([60, 67])).toEqual(["C5", "C power chord"])
    expect(named([55, 60])).toEqual(["G–C", "G–C perfect fourth"])
    expect(named([64, 72])).toEqual(["E–C", "E–C minor sixth"])
    for (let root = 0; root < 12; root++) {
      expect(detectChord([48 + root, 55 + root])).toMatchObject({
        symbol: `${PITCH_NAMES[root]}5`,
        chord: "5",
        root,
      })
    }
  })
})

/*
 * Every quality on C: root position, then one inversion. Where the rule
 * hands an inversion to another root (the symmetric chords), that reading
 * is what is listed.
 */
const QUALITIES: [
  id: string,
  rootPosition: number[],
  symbol: string,
  label: string,
  inversion: number[],
  inversionSymbol: string,
  inversionLabel: string,
][] = [
  ["major", [60, 64, 67], "C", "C major", [64, 67, 72], "C/E", "C major / E"],
  ["minor", [60, 63, 67], "Cm", "C minor", [63, 67, 72], "Cm/D♯", "C minor / D♯"],
  [
    "diminished",
    [60, 63, 66],
    "Cdim",
    "C diminished",
    [63, 66, 72],
    "Cdim/D♯",
    "C diminished / D♯",
  ],
  [
    "augmented",
    [60, 64, 68],
    "Caug",
    "C augmented",
    [64, 68, 72],
    "Eaug",
    "E augmented",
  ],
  [
    "sus2",
    [60, 62, 67],
    "Csus2",
    "C suspended second",
    [62, 67, 72],
    "Csus2/D",
    "C suspended second / D",
  ],
  [
    "sus4",
    [60, 65, 67],
    "Csus4",
    "C suspended fourth",
    [67, 72, 77],
    "Csus4/G",
    "C suspended fourth / G",
  ],
  [
    "6",
    [60, 64, 67, 69],
    "C6",
    "C major sixth",
    [64, 67, 69, 72],
    "C6/E",
    "C major sixth / E",
  ],
  [
    "m6",
    [60, 63, 67, 69],
    "Cm6",
    "C minor sixth",
    [63, 67, 69, 72],
    "Cm6/D♯",
    "C minor sixth / D♯",
  ],
  [
    "69",
    [60, 64, 67, 69, 74],
    "C69",
    "C six-nine",
    [64, 67, 69, 72, 74],
    "C69/E",
    "C six-nine / E",
  ],
  [
    "maj7",
    [60, 64, 67, 71],
    "Cmaj7",
    "C major seventh",
    [64, 67, 71, 72],
    "Cmaj7/E",
    "C major seventh / E",
  ],
  [
    "7",
    [60, 64, 67, 70],
    "C7",
    "C dominant seventh",
    [64, 67, 70, 72],
    "C7/E",
    "C dominant seventh / E",
  ],
  [
    "m7",
    [60, 63, 67, 70],
    "Cm7",
    "C minor seventh",
    [67, 70, 72, 75],
    "Cm7/G",
    "C minor seventh / G",
  ],
  [
    "m(maj7)",
    [60, 63, 67, 71],
    "Cm(maj7)",
    "C minor major seventh",
    [63, 67, 71, 72],
    "Cm(maj7)/D♯",
    "C minor major seventh / D♯",
  ],
  [
    "dim7",
    [60, 63, 66, 69],
    "Cdim7",
    "C diminished seventh",
    [63, 66, 69, 72],
    "D♯dim7",
    "D♯ diminished seventh",
  ],
  [
    "m7♭5",
    [60, 63, 66, 70],
    "Cm7♭5",
    "C half-diminished seventh",
    [66, 70, 72, 75],
    "Cm7♭5/F♯",
    "C half-diminished seventh / F♯",
  ],
  [
    "aug7",
    [60, 64, 68, 70],
    "Caug7",
    "C augmented seventh",
    [64, 68, 70, 72],
    "Caug7/E",
    "C augmented seventh / E",
  ],
  [
    "maj7♯5",
    [60, 64, 68, 71],
    "Cmaj7♯5",
    "C augmented major seventh",
    [64, 68, 71, 72],
    "Cmaj7♯5/E",
    "C augmented major seventh / E",
  ],
  [
    "7sus4",
    [60, 65, 67, 70],
    "C7sus4",
    "C dominant seventh suspended fourth",
    [65, 67, 70, 72],
    "C7sus4/F",
    "C dominant seventh suspended fourth / F",
  ],
  [
    "7♭5",
    [60, 64, 66, 70],
    "C7♭5",
    "C dominant seventh flat five",
    [64, 66, 70, 72],
    "C7♭5/E",
    "C dominant seventh flat five / E",
  ],
  [
    "add9",
    [60, 64, 67, 74],
    "Cadd9",
    "C added ninth",
    [64, 67, 72, 74],
    "Cadd9/E",
    "C added ninth / E",
  ],
  [
    "m(add9)",
    [60, 63, 67, 74],
    "Cm(add9)",
    "C minor added ninth",
    [63, 67, 72, 74],
    "Cm(add9)/D♯",
    "C minor added ninth / D♯",
  ],
  [
    "add11",
    [60, 64, 67, 77],
    "Cadd11",
    "C added eleventh",
    [64, 65, 67, 72],
    "Cadd11/E",
    "C added eleventh / E",
  ],
  [
    "m(add11)",
    [60, 63, 67, 77],
    "Cm(add11)",
    "C minor added eleventh",
    [63, 65, 67, 72],
    "Cm(add11)/D♯",
    "C minor added eleventh / D♯",
  ],
  [
    "maj9",
    [60, 64, 67, 71, 74],
    "Cmaj9",
    "C major ninth",
    [64, 67, 71, 72, 74],
    "Cmaj9/E",
    "C major ninth / E",
  ],
  [
    "9",
    [60, 64, 67, 70, 74],
    "C9",
    "C dominant ninth",
    [64, 67, 70, 72, 74],
    "C9/E",
    "C dominant ninth / E",
  ],
  [
    "m9",
    [60, 63, 67, 70, 74],
    "Cm9",
    "C minor ninth",
    [63, 67, 70, 72, 74],
    "Cm9/D♯",
    "C minor ninth / D♯",
  ],
  [
    "7♭9",
    [60, 64, 67, 70, 73],
    "C7♭9",
    "C dominant seventh flat nine",
    [64, 67, 70, 72, 73],
    "C7♭9/E",
    "C dominant seventh flat nine / E",
  ],
  [
    "7♯9",
    [60, 64, 67, 70, 75],
    "C7♯9",
    "C dominant seventh sharp nine",
    [64, 67, 70, 72, 75],
    "C7♯9/E",
    "C dominant seventh sharp nine / E",
  ],
  [
    "11",
    [60, 64, 67, 70, 74, 77],
    "C11",
    "C dominant eleventh",
    [64, 67, 70, 72, 74, 77],
    "C11/E",
    "C dominant eleventh / E",
  ],
  [
    "m11",
    [60, 63, 67, 70, 74, 77],
    "Cm11",
    "C minor eleventh",
    [63, 67, 70, 72, 74, 77],
    "Cm11/D♯",
    "C minor eleventh / D♯",
  ],
  [
    "13",
    [60, 64, 67, 70, 74, 77, 81],
    "C13",
    "C dominant thirteenth",
    [64, 67, 70, 72, 74, 77, 81],
    "C13/E",
    "C dominant thirteenth / E",
  ],
]

describe("chord vocabulary", () => {
  it("lists every quality the detector knows", () => {
    expect(QUALITIES.map(([id]) => id)).toEqual(CHORDS.map(({ id }) => id))
  })

  it.each(QUALITIES)(
    "names %s in root position",
    (id, keys, symbol, label) => {
      const template = CHORDS.find((chord) => chord.id === id)!
      const triad = keys.length === 3
      expect(detectChord(keys)).toEqual({
        kind: triad ? "triad" : "chord",
        label,
        symbol,
        root: 0,
        bass: 0,
        quality: triad ? id : null,
        chord: id,
        pitchClasses: [...new Set(keys.map(pitchClass))].sort((a, b) => a - b),
        intervals: template.intervals,
        degrees: template.degrees,
        omitted: [],
      })
    }
  )

  it.each(QUALITIES)(
    "names an inversion of %s by its bass",
    (_id, _keys, _symbol, _label, inversion, symbol, label) => {
      const detection = detectChord(inversion)
      expect([detection.symbol, detection.label]).toEqual([symbol, label])
      expect(detection.bass).toBe(pitchClass(Math.min(...inversion)))
    }
  )

  it.each([
    "major",
    "minor",
    "diminished",
    "sus4",
    "6",
    "maj7",
    "7",
    "m7",
    "m(maj7)",
    "dim7",
    "m7♭5",
    "aug7",
    "7sus4",
    "add9",
    "maj9",
    "9",
    "m9",
    "7♭9",
    "7♯9",
    "11",
    "13",
  ])("names %s in root position on all twelve roots", (id) => {
    const template = CHORDS.find((chord) => chord.id === id)!
    for (let root = 0; root < 12; root++) {
      // Extensions sit above the octave, as they are usually voiced.
      const keys = template.intervals.map((interval, index) =>
        index > 3 ? 60 + root + interval : 48 + root + interval
      )
      expect(detectChord(keys), `${PITCH_NAMES[root]}${template.suffix}`).toMatchObject({
        root,
        bass: root,
        chord: id,
        symbol: `${PITCH_NAMES[root]}${template.suffix}`,
        label: `${PITCH_NAMES[root]} ${template.name}`,
        omitted: [],
      })
    }
  })

  it("names black keys with sharps, whatever the chord would spell them", () => {
    expect(named([63, 67, 70])).toEqual(["D♯", "D♯ major"])
    expect(named([58, 61, 65])).toEqual(["A♯m", "A♯ minor"])
    expect(named([66, 70, 73, 76])).toEqual(["F♯7", "F♯ dominant seventh"])
    expect(detectChord([60, 63, 66, 70]).pitchClasses.map((pc) => PITCH_NAMES[pc])).toEqual(
      ["C", "D♯", "F♯", "A♯"]
    )
  })
})

describe("omitted tones", () => {
  it.each([
    [[60, 64, 71], "Cmaj7", [7]],
    [[60, 64, 70], "C7", [7]],
    [[60, 63, 70], "Cm7", [7]],
    [[60, 63, 71], "Cm(maj7)", [7]],
    [[60, 64, 71, 74], "Cmaj9", [7]],
    [[60, 64, 70, 74], "C9", [7]],
    [[60, 63, 70, 74], "Cm9", [7]],
    [[60, 64, 70, 73], "C7♭9", [7]],
    [[60, 64, 70, 75], "C7♯9", [7]],
    [[60, 64, 70, 74, 77], "C11", [7]],
    [[60, 64, 67, 70, 77], "C11", [2]],
    [[60, 64, 70, 77], "C11", [7, 2]],
    [[60, 63, 70, 74, 77], "Cm11", [7]],
    [[60, 63, 70, 77], "Cm11", [7, 2]],
    [[60, 64, 70, 74, 81], "C13", [7, 5]],
    [[60, 64, 67, 70, 81], "C13", [2, 5]],
    [[60, 64, 70, 81], "C13", [7, 2, 5]],
    // Over another bass the fifth may still be missing.
    [[64, 70, 72], "C7/E", [7]],
    [[70, 72, 76, 86], "C9/A♯", [7]],
  ])("names %j as %s and reports what is left out", (keys, symbol, omitted) => {
    const detection = detectChord(keys)
    expect(detection.symbol).toBe(symbol)
    expect(detection.omitted).toEqual(omitted)
    expect(detection.intervals).not.toEqual(
      expect.arrayContaining(omitted as number[])
    )
    expect(detection.degrees).toHaveLength(detection.intervals.length)
  })

  it("does not drop the fifth of a triad, a sixth, a suspension or an altered fifth", () => {
    // Without G these are other chords, never C6, Cm6 or C7sus4.
    expect(named([60, 64, 69])).toEqual(["Am/C", "A minor / C"])
    expect(named([60, 63, 69])).toEqual(["Adim/C", "A diminished / C"])
    expect(named([60, 65, 70])).toEqual(["Fsus4/C", "F suspended fourth / C"])
    expect(detectChord([60, 64]).kind).toBe("interval")
    expect(detectChord([60, 62, 64]).kind).toBe("unknown")
    // A chord may never lose its root or the tone it is named for.
    expect(detectChord([64, 67, 70]).symbol).toBe("Edim")
    expect(detectChord([60, 64, 67, 74]).symbol).toBe("Cadd9")
  })
})

describe("ambiguous sets", () => {
  it("lets the bass decide between C major sixth and A minor seventh", () => {
    expect(named([60, 64, 67, 69])).toEqual(["C6", "C major sixth"])
    expect(named([57, 60, 64, 67])).toEqual(["Am7", "A minor seventh"])
    // Neither root in the bass: the lower pitch class, C, is the root.
    expect(named([64, 67, 69, 72])).toEqual(["C6/E", "C major sixth / E"])
    expect(named([67, 69, 72, 76])).toEqual(["C6/G", "C major sixth / G"])
    // The same shape a major sixth lower: F♯ is below A.
    expect(named([57, 61, 64, 66])).toEqual(["A6", "A major sixth"])
    expect(named([54, 57, 61, 64])).toEqual(["F♯m7", "F♯ minor seventh"])
    expect(named([61, 64, 66, 69])).toEqual([
      "F♯m7/C♯",
      "F♯ minor seventh / C♯",
    ])
  })

  it("lets the bass decide between a minor sixth and a half-diminished seventh", () => {
    expect(named([60, 63, 67, 69])).toEqual(["Cm6", "C minor sixth"])
    expect(named([57, 60, 63, 67])).toEqual([
      "Am7♭5",
      "A half-diminished seventh",
    ])
    expect(named([63, 67, 69, 72])).toEqual(["Cm6/D♯", "C minor sixth / D♯"])
    expect(named([62, 65, 71, 81])).toEqual(["Dm6", "D minor sixth"])
    expect(named([59, 62, 65, 69])).toEqual([
      "Bm7♭5",
      "B half-diminished seventh",
    ])
    expect(named([65, 69, 71, 74])).toEqual(["Dm6/F", "D minor sixth / F"])
  })

  it("lets the bass decide between suspended seconds and fourths", () => {
    expect(named([60, 62, 67])).toEqual(["Csus2", "C suspended second"])
    expect(named([55, 60, 62])).toEqual(["Gsus4", "G suspended fourth"])
    expect(named([62, 67, 72])).toEqual(["Csus2/D", "C suspended second / D"])
    expect(named([60, 65, 67])).toEqual(["Csus4", "C suspended fourth"])
    expect(named([53, 55, 60])).toEqual(["Fsus2", "F suspended second"])
  })

  it("roots a symmetric chord on its bass", () => {
    for (const bass of [60, 63, 66, 69]) {
      const keys = [60, 63, 66, 69].map((key) => (key < bass ? key + 12 : key))
      expect(detectChord(keys)).toMatchObject({
        root: pitchClass(bass),
        chord: "dim7",
        symbol: `${PITCH_NAMES[pitchClass(bass)]}dim7`,
      })
    }
    for (const bass of [60, 64, 68]) {
      const keys = [60, 64, 68].map((key) => (key < bass ? key + 12 : key))
      expect(detectChord(keys)).toMatchObject({
        root: pitchClass(bass),
        quality: "augmented",
      })
    }
    // A flat-five dominant is its own tritone substitute.
    expect(named([60, 64, 66, 70])[0]).toBe("C7♭5")
    expect(named([66, 70, 72, 76])[0]).toBe("F♯7♭5")
    expect(named([64, 66, 70, 72])[0]).toBe("C7♭5/E")
    expect(named([70, 72, 76, 78])[0]).toBe("C7♭5/A♯")
  })

  it("prefers the bass as root, then the fewest omissions, then the lowest root", () => {
    // C–D–E–G–A: six-nine on C, minor eleventh without its ninth on A.
    expect(named([60, 62, 64, 67, 69])[0]).toBe("C69")
    expect(named([57, 60, 62, 64, 67])[0]).toBe("Am11")
    expect(named([62, 64, 67, 69, 72])[0]).toBe("C69/D")
    // C–E♭–F–B♭: complete on F, two tones short on C.
    expect(named([60, 63, 65, 70])[0]).toBe("Cm11")
    expect(named([53, 60, 63, 70])[0]).toBe("F7sus4")
    expect(named([63, 65, 70, 72])[0]).toBe("F7sus4/D♯")
    // A minor seventh over its third is a major sixth on that third.
    expect(named([63, 67, 70, 72])[0]).toBe("D♯6")
    expect(named([67, 70, 72, 75])[0]).toBe("Cm7/G")
  })

  it("ranks every reading of every pitch-class set by the documented rule", () => {
    // An independent enumeration with sets instead of the detector's bit masks.
    for (let set = 1; set < 4096; set++) {
      const pcs = Array.from({ length: 12 }, (_, pc) => pc).filter(
        (pc) => set & (1 << pc)
      )
      if (pcs.length < 3) continue
      for (const bass of pcs) {
        const keys = pcs.map((pc) => (pc < bass ? 72 + pc : 60 + pc))
        const readings: [number, number, number, number][] = []
        for (const root of pcs) {
          const above = new Set(pcs.map((pc) => pitchClass(pc - root)))
          CHORDS.forEach((template, index) => {
            const optional = template.optional ?? []
            const missing = template.intervals.filter((i) => !above.has(i))
            if (
              [...above].every((i) => template.intervals.includes(i)) &&
              missing.every((i) => optional.includes(i))
            ) {
              readings.push([
                root === bass ? 0 : 1,
                missing.length + (template.alterations ?? 0),
                root,
                index,
              ])
            }
          })
        }
        readings.sort(
          (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2] || a[3] - b[3]
        )
        const detection = detectChord(keys)
        const context = `${pcs.join(",")} over ${bass}`
        if (!readings.length) {
          expect(detection.kind, context).toBe("unknown")
          continue
        }
        const [, , root, index] = readings[0]
        expect([detection.root, detection.chord], context).toEqual([
          root,
          CHORDS[index].id,
        ])
        expect(detection.bass, context).toBe(bass)
        expect(detection.symbol.includes("/"), context).toBe(root !== bass)
        // Reversed and doubled above the bass, it is the same chord.
        expect(
          detectChord([...keys.map((key) => key + 12), ...keys].reverse()),
          context
        ).toEqual(detection)
      }
    }
  })
})

describe("input handling", () => {
  it("ignores order and octave doublings above the bass, and leaves the input alone", () => {
    const keys = [86, 71, 48, 64, 79, 60, 67]
    const copy = [...keys]
    expect(detectChord(keys)).toEqual(detectChord([48, 52, 55, 59, 62]))
    expect(keys).toEqual(copy)
    for (const rotation of [0, 1, 2, 3]) {
      const voicing = [60, 64, 67, 70]
      const rotated = [...voicing.slice(rotation), ...voicing.slice(0, rotation)]
      expect(detectChord(rotated)).toEqual(detectChord(voicing))
    }
  })

  it("skips keys that are not whole numbers, also when finding the bass", () => {
    expect(detectChord([NaN, 59.5, 60, 64, 67, Infinity])).toEqual(
      detectChord([60, 64, 67])
    )
    expect(detectChord([NaN, 0.5])).toMatchObject({
      kind: "empty",
      label: "No notes selected",
      symbol: "",
      bass: null,
    })
  })

  it("names keys below zero by their pitch class", () => {
    expect(detectChord([-12, -8, -5]).symbol).toBe("C")
  })
})

describe("unknown sets", () => {
  it.each([
    [[60, 61, 67], "Unknown chord (C, C♯, G)", 0, [0, 1, 7], ["1", "♭2", "5"]],
    [[60, 61, 62], "Unknown chord (C, C♯, D)", 0, [0, 1, 2], ["1", "♭2", "2"]],
    [[66, 72, 77], "Unknown chord (C, F, F♯)", 6, [0, 6, 11], ["1", "♭5", "7"]],
    [
      [60, 61, 62, 63, 64],
      "Unknown chord (C, C♯, D, D♯, E)",
      0,
      [0, 1, 2, 3, 4],
      ["1", "♭2", "2", "♭3", "3"],
    ],
  ])("lists the pitch classes of %j", (keys, label, bass, intervals, degrees) => {
    expect(detectChord(keys)).toEqual({
      kind: "unknown",
      label,
      symbol: "?",
      root: null,
      bass,
      quality: null,
      chord: null,
      pitchClasses: [...new Set(keys.map(pitchClass))].sort((a, b) => a - b),
      intervals,
      degrees,
      omitted: [],
    })
  })

  it("lists all twelve pitch classes of a chromatic cluster", () => {
    expect(
      detectChord(Array.from({ length: 12 }, (_, key) => 60 + key)).label
    ).toBe(`Unknown chord (${PITCH_NAMES.join(", ")})`)
  })
})
