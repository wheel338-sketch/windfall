import { describe, expect, it } from "vitest"

import type { Command, Note, Project } from "@/bindings"
import { SimDocument } from "@/lib/ipc/sim/document"
import { demoProject } from "@/lib/ipc/sim/project"
import { historyLabel, historyLabelText } from "@/lib/i18n"

import { fitPitch, LEFT_WIDTH, MAX_STEP_PITCH, MIN_STEP_PITCH } from "./layout"
import {
  clampPatternLength,
  describeLength,
  detailSteps,
  fillCommand,
  fillSteps,
  isStepNote,
  joinTune,
  litSteps,
  moveIndex,
  rotateStart,
  rotateSteps,
  rulerMarks,
  shiftCommand,
  splitTune,
  stepNote,
  stepsPerBar,
  stepsPerBeat,
} from "./steps"

let nextId = 100
function note(start: number, patch: Partial<Note> = {}): Note {
  nextId += 1
  return {
    id: nextId,
    start,
    length: 240,
    key: 60,
    velocity: 0.8,
    pan: 0,
    ...patch,
  }
}

const row = (steps: boolean[]) => steps.map((lit) => (lit ? "x" : ".")).join("")

/** Runs a command on a document, and reads back the project and the label. */
function applyCommand(project: Project, command: Command) {
  const document = SimDocument.create(project)
  try {
    const { patch } = document.dispatch(command)
    const label = patch.history.entries.at(-1)?.label
    return { project: document.project(), label }
  } finally {
    document.dispose()
  }
}

describe("steps and notes", () => {
  it("makes the note a lit step stands for", () => {
    expect(stepNote(0)).toEqual({ start: 0, length: 240, key: 60 })
    expect(stepNote(5)).toEqual({ start: 1200, length: 240, key: 60 })
  })

  it("lights a step only when a note starts exactly on it", () => {
    const notes = [note(0), note(960), note(1000), note(3600)]
    expect(row(litSteps(notes, 16))).toBe("x...x..........x")
  })

  it("lights a step for a note on any key", () => {
    expect(row(litSteps([note(240, { key: 72 })], 4))).toBe(".x..")
  })

  it("ignores notes at or past the end of the pattern", () => {
    expect(row(litSteps([note(0), note(16 * 240)], 16))).toBe(
      "x..............."
    )
    expect(row(litSteps([note(0), note(16 * 240)], 32))).toBe(
      "x...............x..............."
    )
  })

  it("gives an empty row for a channel with no lane", () => {
    expect(row(litSteps(undefined, 4))).toBe("....")
    expect(detailSteps(undefined, 4)).toEqual([])
  })

  it("tells a plain step note from piano roll data", () => {
    expect(isStepNote(note(480))).toBe(true)
    expect(isStepNote(note(480, { key: 61 }))).toBe(false)
    expect(isStepNote(note(480, { length: 480 }))).toBe(false)
    expect(isStepNote(note(500))).toBe(false)
    // Velocity and pan are still a plain step.
    expect(isStepNote(note(480, { velocity: 0.3, pan: -1 }))).toBe(true)
  })

  it("marks the steps that hold what a toggle cannot show", () => {
    const notes = [
      note(0),
      note(240, { key: 64 }),
      note(500),
      note(960),
      note(960, { key: 67 }),
      note(1440, { length: 960 }),
      note(1680),
      note(1680),
      note(99 * 240, { key: 1 }),
    ]
    // 1: other key. 2: off the grid. 4: a chord. 6: long. 7: two notes.
    expect(detailSteps(notes, 16)).toEqual([1, 2, 4, 6, 7])
  })

  it("has no marks for a row made only by clicking steps", () => {
    expect(detailSteps([note(0), note(960), note(1920)], 16)).toEqual([])
  })
})

describe("fill", () => {
  it("lists every Nth step", () => {
    expect(fillSteps(4, 16)).toEqual([0, 4, 8, 12])
    expect(fillSteps(8, 16)).toEqual([0, 8])
    expect(fillSteps(2, 5)).toEqual([0, 2, 4])
    expect(fillSteps(4, 3)).toEqual([0])
  })

  it("replaces the row inside the pattern, as one labelled batch", () => {
    const inside = note(240)
    const outside = note(16 * 240)
    const command = fillCommand(1, 7, [inside, outside], 16, 4)
    expect(command).toEqual({
      type: "batch",
      // Kept as its key and value; the history makes the words when shown.
      label: historyLabel("Fill every {every} steps", { every: 4 }),
      commands: [
        { type: "removeNotes", pattern: 1, channel: 7, notes: [inside.id] },
        {
          type: "addNotes",
          pattern: 1,
          channel: 7,
          notes: [0, 4, 8, 12].map(stepNote),
        },
      ],
    })
  })

  it("only adds when the row is empty", () => {
    const command = fillCommand(1, 7, undefined, 8, 2)
    expect(command.type === "batch" && command.commands).toHaveLength(1)
  })

  it("gives the right row when applied to a project", () => {
    const project = demoProject()
    const pattern = project.patterns[0]
    const hat = project.channels[2].id
    const lane = pattern.lanes.find((item) => item.channel === hat)
    const applied = applyCommand(
      project,
      fillCommand(pattern.id, hat, lane?.notes, pattern.lengthSteps, 8)
    )
    const after = applied.project.patterns[0].lanes.find(
      (item) => item.channel === hat
    )
    expect(row(litSteps(after?.notes, 16))).toBe("x.......x.......")
    expect(historyLabelText(applied.label)).toBe("Fill every 8 steps")
  })
})

describe("shift", () => {
  it("wraps a start around the pattern", () => {
    expect(rotateStart(0, 1, 16)).toBe(240)
    expect(rotateStart(15 * 240, 1, 16)).toBe(0)
    expect(rotateStart(0, -1, 16)).toBe(15 * 240)
    // A note between steps keeps its distance from the step.
    expect(rotateStart(15 * 240 + 100, 1, 16)).toBe(100)
  })

  it("rotates a row of steps", () => {
    const steps = [true, false, false, true]
    expect(row(rotateSteps(steps, 1))).toBe("xx..")
    expect(row(rotateSteps(steps, -1))).toBe("..xx")
    expect(rotateSteps([], 1)).toEqual([])
  })

  it("moves only the notes inside the pattern, as one labelled batch", () => {
    const first = note(0)
    const last = note(15 * 240, { key: 64 })
    const outside = note(20 * 240)
    const command = shiftCommand(1, 7, [first, last, outside], 16, 1)
    expect(command).toEqual({
      type: "batch",
      label: "Shift steps right",
      commands: [
        {
          type: "updateNotes",
          pattern: 1,
          channel: 7,
          updates: [
            { id: first.id, patch: { start: 240 } },
            { id: last.id, patch: { start: 0 } },
          ],
        },
      ],
    })
    expect(shiftCommand(1, 7, [first], 16, -1)).toMatchObject({
      label: "Shift steps left",
    })
  })

  it("has nothing to do for an empty row or a one-step pattern", () => {
    expect(shiftCommand(1, 7, undefined, 16, 1)).toBeNull()
    expect(shiftCommand(1, 7, [note(16 * 240)], 16, 1)).toBeNull()
    expect(shiftCommand(1, 7, [note(0)], 1, 1)).toBeNull()
  })

  it("matches rotating the lit row when applied to a project", () => {
    const project = demoProject()
    const pattern = project.patterns[0]
    const snare = project.channels[3].id
    const lane = pattern.lanes.find((item) => item.channel === snare)
    const before = litSteps(lane?.notes, 16)
    const command = shiftCommand(pattern.id, snare, lane?.notes, 16, 1)
    const applied = applyCommand(project, command as Command)
    const after = applied.project.patterns[0].lanes.find(
      (item) => item.channel === snare
    )
    // The snare sits on steps 7 and 15, so the last one wraps to the start.
    expect(row(litSteps(after?.notes, 16))).toBe("x.......x.......")
    expect(litSteps(after?.notes, 16)).toEqual(rotateSteps(before, 1))
  })
})

describe("ruler", () => {
  const fourFour = { numerator: 4, denominator: 4 }

  it("counts steps per beat and bar from the time signature", () => {
    expect(stepsPerBeat(fourFour)).toBe(4)
    expect(stepsPerBar(fourFour)).toBe(16)
    expect(stepsPerBeat({ numerator: 6, denominator: 8 })).toBe(2)
    expect(stepsPerBar({ numerator: 6, denominator: 8 })).toBe(12)
    expect(stepsPerBar({ numerator: 3, denominator: 4 })).toBe(12)
  })

  it("marks every beat and numbers the bars", () => {
    expect(rulerMarks(32, fourFour)).toEqual([
      { step: 0, label: "1", bar: true },
      { step: 4, label: "1.2", bar: false },
      { step: 8, label: "1.3", bar: false },
      { step: 12, label: "1.4", bar: false },
      { step: 16, label: "2", bar: true },
      { step: 20, label: "2.2", bar: false },
      { step: 24, label: "2.3", bar: false },
      { step: 28, label: "2.4", bar: false },
    ])
  })

  it("follows a three-four bar", () => {
    const marks = rulerMarks(24, { numerator: 3, denominator: 4 })
    expect(marks.filter((mark) => mark.bar).map((mark) => mark.step)).toEqual([
      0, 12,
    ])
  })

  it("says how long a pattern is", () => {
    expect(describeLength(16, fourFour)).toBe("1 bar")
    expect(describeLength(64, fourFour)).toBe("4 bars")
    expect(describeLength(12, fourFour)).toBe("12 steps")
    expect(describeLength(17, fourFour)).toBe("1 bar and 1 step")
    expect(describeLength(36, fourFour)).toBe("2 bars and 4 steps")
  })

  it("keeps a pattern length whole and in range", () => {
    expect(clampPatternLength(0)).toBe(1)
    expect(clampPatternLength(16.4)).toBe(16)
    expect(clampPatternLength(5000)).toBe(1024)
  })
})

describe("reordering", () => {
  it("turns a gap between rows into a moveChannel index", () => {
    // Four rows; the gaps are numbered 0 (above the first) to 4 (below the last).
    expect(moveIndex(0, 2, 4)).toBe(1)
    expect(moveIndex(0, 4, 4)).toBe(3)
    expect(moveIndex(3, 0, 4)).toBe(0)
    expect(moveIndex(2, 1, 4)).toBe(1)
  })

  it("is null for the gaps on either side of the row itself", () => {
    expect(moveIndex(1, 1, 4)).toBeNull()
    expect(moveIndex(1, 2, 4)).toBeNull()
    expect(moveIndex(0, 0, 1)).toBeNull()
  })
})

describe("tuning", () => {
  it("splits a tuning into semitones and cents and joins it again", () => {
    expect(splitTune(0)).toEqual({ semitones: 0, cents: 0 })
    expect(splitTune(7.25)).toEqual({ semitones: 7, cents: 25 })
    expect(splitTune(-3.4)).toEqual({ semitones: -3, cents: -40 })
    expect(splitTune(11.9)).toEqual({ semitones: 12, cents: -10 })
    expect(joinTune(7, 25, 48)).toBeCloseTo(7.25)
    expect(joinTune(-3, -40, 48)).toBeCloseTo(-3.4)
  })

  it("reads a tuning half way between two semitones as the one nearer zero", () => {
    expect(splitTune(0.5)).toEqual({ semitones: 0, cents: 50 })
    expect(splitTune(-0.5)).toEqual({ semitones: 0, cents: -50 })
    expect(splitTune(1.5)).toEqual({ semitones: 1, cents: 50 })
    expect(splitTune(-1.5)).toEqual({ semitones: -1, cents: -50 })
    // Zero is plain zero, whichever side it was reached from.
    expect(Object.is(splitTune(-0.2).semitones, 0)).toBe(true)
    expect(Object.is(splitTune(-3).cents, 0)).toBe(true)
  })

  it("keeps the semitone on show for as long as the tuning is within 50 cents of it", () => {
    // Fine all the way down on semitone 1 is 0.5, and stays "1, -50".
    expect(splitTune(0.5, 1)).toEqual({ semitones: 1, cents: -50 })
    expect(splitTune(0.5, 0)).toEqual({ semitones: 0, cents: 50 })
    expect(splitTune(1.5, 2)).toEqual({ semitones: 2, cents: -50 })
    expect(splitTune(7.25, 7)).toEqual({ semitones: 7, cents: 25 })
    // Further away than that, the semitone on show is out of date.
    expect(splitTune(0.5, 2)).toEqual({ semitones: 0, cents: 50 })
    expect(splitTune(3.2, 1)).toEqual({ semitones: 3, cents: 20 })
    // Every pair it gives joins back into the tuning it came from.
    for (const [value, prefer] of [
      [0.5, 1],
      [0.5, 0],
      [-11.5, -12],
      [2.49, 2],
    ]) {
      const { semitones, cents } = splitTune(value, prefer)
      expect(joinTune(semitones, cents, 48)).toBeCloseTo(value, 9)
      expect(Math.abs(cents)).toBeLessThanOrEqual(50)
    }
  })

  it("never leaves the range", () => {
    expect(joinTune(48, 30, 48)).toBe(48)
    expect(joinTune(-48, -30, 48)).toBe(-48)
  })
})

describe("step width", () => {
  it("uses spare room up to a limit and never goes below the minimum", () => {
    expect(fitPitch(LEFT_WIDTH + 2000, 16)).toBe(MAX_STEP_PITCH)
    expect(fitPitch(LEFT_WIDTH + 100, 64)).toBe(MIN_STEP_PITCH)
    const between = fitPitch(LEFT_WIDTH + 18 + 16 * 25, 16)
    expect(between).toBe(25)
  })
})
