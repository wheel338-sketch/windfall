import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

import type { EffectKind } from "@/bindings"
import { SimDocument } from "@/lib/ipc/sim/document"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"

import {
  effectOutputChoices,
  effectOutputsOf,
  EffectOutputs,
  withEffectOutputEdges,
} from "./effect-outputs"
import { flush, history, project, trackNamed } from "./test-utils"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  app = await startTestApp()
})
afterEach(() => app.stop())

const routes = () => project().mixer.effectOutputs ?? []

async function fileRoundTrip() {
  const snapshot = await app.backend.documentSnapshot()
  const doc = SimDocument.create(snapshot.project)
  const reopened = SimDocument.open(doc.fileText())
  expect(reopened.project()).toEqual(snapshot.project)
  doc.dispose()
  reopened.dispose()
}

async function addEffect(track: number, kind: EffectKind): Promise<number> {
  const result = await dispatch({ type: "addEffect", track, kind })
  if (!result) throw new Error(`Could not add ${kind}`)
  return result.created[0]
}

/** The routing rows of one effect, read from the store like the inspector. */
function Outputs({ track, effect }: { track: number; effect: number }) {
  const slot = useProjectStore((state) =>
    state.project.mixer.tracks
      .find((item) => item.id === track)
      ?.effects.find((item) => item.id === effect)
  )
  return slot ? <EffectOutputs track={track} slot={slot} /> : null
}

async function choose(
  user: ReturnType<typeof userEvent.setup>,
  button: string,
  item: string | RegExp
) {
  await user.click(screen.getByRole("button", { name: button }))
  await user.click(await screen.findByRole("menuitemradio", { name: item }))
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
  await flush()
}

describe("effect output routing in the inspector", () => {
  it("lists outputs from the engine's table: one copy for Send Tap, two parts for Transient Split, none for the rest", () => {
    expect(effectOutputsOf("sendTap")).toEqual([
      { id: "send", label: "Send", splits: false },
    ])
    expect(effectOutputsOf("transientSplit")).toEqual([
      { id: "transient", label: "Transient", splits: true },
      { id: "sustain", label: "Sustain", splits: true },
    ])
    for (const kind of ["eq", "compressor", "bandSplit", "reverb"]) {
      expect(effectOutputsOf(kind)).toEqual([])
    }
  })

  it("routes a Send Tap to another track, retargets and disconnects it, one undo step each, and saves it", async () => {
    const user = userEvent.setup()
    const kick = trackNamed("Kick").id
    const clap = trackNamed("Clap").id
    const hat = trackNamed("Hat").id
    const tap = await addEffect(kick, "sendTap")
    const before = structuredClone(project())
    const cursor = history().cursor
    render(<Outputs track={kick} effect={tap} />)

    const unrouted = "Send output of Send Tap: not routed"
    expect(screen.getByRole("button", { name: unrouted })).toHaveTextContent(
      "Not routed"
    )
    await user.click(screen.getByRole("button", { name: unrouted }))
    // Its own track is not a destination.
    expect(
      await screen.findByRole("menuitemradio", { name: /Clap/ })
    ).toBeVisible()
    expect(screen.queryByRole("menuitemradio", { name: /Kick/ })).toBeNull()
    expect(
      screen.getByRole("menuitemradio", { name: "Not routed" })
    ).toHaveAttribute("aria-checked", "true")
    await user.click(screen.getByRole("menuitemradio", { name: /Clap/ }))
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    await flush()
    expect(routes()).toEqual([{ effect: tap, output: 0, target: clap }])
    expect(history().cursor).toBe(cursor + 1)
    expect(
      screen.getByRole("button", { name: "Send output of Send Tap: Clap" })
    ).toHaveTextContent("Clap")
    // Nothing else of the project moved.
    expect(project().mixer.tracks).toEqual(before.mixer.tracks)
    expect(project().channels).toEqual(before.channels)
    await fileRoundTrip()

    await choose(user, "Send output of Send Tap: Clap", /Hat/)
    expect(routes()).toEqual([{ effect: tap, output: 0, target: hat }])
    expect(history().cursor).toBe(cursor + 2)

    await choose(user, "Send output of Send Tap: Hat", "Not routed")
    expect(routes()).toEqual([])
    expect(history().cursor).toBe(cursor + 3)
    expect(screen.getByRole("button", { name: unrouted })).toBeVisible()

    await act(async () => {
      await undo()
    })
    expect(routes()).toEqual([{ effect: tap, output: 0, target: hat }])
    await act(async () => {
      await undo()
      await undo()
    })
    expect(project()).toEqual(before)
    await act(async () => {
      await redo()
    })
    expect(routes()).toEqual([{ effect: tap, output: 0, target: clap }])
    expect(
      screen.getByRole("button", { name: "Send output of Send Tap: Clap" })
    ).toBeVisible()
  })

  it.each(["transientSplit", "transientShaper"] as const)("routes the two parts of %s separately and retires the routes with the effect", async (kind) => {
    const name = kind === "transientSplit" ? "Transient Split" : "Transient Shaper"
    const user = userEvent.setup()
    const kick = trackNamed("Kick").id
    const clap = trackNamed("Clap").id
    const hat = trackNamed("Hat").id
    const split = await addEffect(kick, kind)
    render(<Outputs track={kick} effect={split} />)
    expect(
      screen.getByRole("list", { name: `${name} output routing` })
    ).toBeVisible()

    await choose(
      user,
      `Transient output of ${name}: not routed`,
      /Clap/
    )
    await choose(user, `Sustain output of ${name}: not routed`, /Hat/)
    expect(routes()).toEqual([
      { effect: split, output: 0, target: clap },
      { effect: split, output: 1, target: hat },
    ])
    expect(
      screen.getByRole("button", {
        name: `Transient output of ${name}: Clap`,
      })
    ).toBeVisible()
    expect(
      screen.getByRole("button", {
        name: `Sustain output of ${name}: Hat`,
      })
    ).toBeVisible()
    // A part that is not routed stays in the chain, and the menu says so.
    await user.click(
      screen.getByRole("button", {
        name: `Sustain output of ${name}: Hat`,
      })
    )
    expect(
      await screen.findByRole("menuitemradio", {
        name: "Not routed (stays in the chain)",
      })
    ).toBeVisible()
    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    await fileRoundTrip()

    // Deleting the destination of one part retires that route only.
    const routed = structuredClone(project())
    await act(async () => {
      await dispatch({ type: "removeMixerTrack", id: hat })
    })
    await flush()
    expect(routes()).toEqual([{ effect: split, output: 0, target: clap }])
    expect(
      screen.getByRole("button", {
        name: `Sustain output of ${name}: not routed`,
      })
    ).toBeVisible()
    await act(async () => {
      await undo()
    })
    expect(project()).toEqual(routed)

    // Deleting the effect retires both, and one undo restores both.
    await act(async () => {
      await dispatch({ type: "removeEffect", track: kick, effect: split })
    })
    await flush()
    expect(routes()).toEqual([])
    expect(screen.queryByRole("list")).toBeNull()
    await act(async () => {
      await undo()
    })
    expect(project()).toEqual(routed)
    expect(
      screen.getByRole("button", {
        name: `Transient output of ${name}: Clap`,
      })
    ).toBeVisible()
  })

  it("leaves out destinations that would loop, and the backend refuses loops in both directions without editing", async () => {
    const user = userEvent.setup()
    const kick = trackNamed("Kick").id
    const clap = trackNamed("Clap").id
    const hat = trackNamed("Hat").id
    const tap = await addEffect(kick, "sendTap")
    // Clap already plays into Kick, so Kick's tap cannot feed Clap.
    await dispatch({ type: "setTrackOutput", id: clap, output: kick })
    render(<Outputs track={kick} effect={tap} />)
    await user.click(
      screen.getByRole("button", { name: "Send output of Send Tap: not routed" })
    )
    expect(
      await screen.findByRole("menuitemradio", { name: /Hat/ })
    ).toBeVisible()
    expect(screen.queryByRole("menuitemradio", { name: /Clap/ })).toBeNull()
    expect(screen.getByText(/1 track is not listed/)).toBeVisible()
    await user.click(screen.getByRole("menuitemradio", { name: /Hat/ }))
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    await flush()
    expect(routes()).toEqual([{ effect: tap, output: 0, target: hat }])

    const before = structuredClone(project())
    const cursor = history().cursor
    const refused = [
      // The tap into a track that feeds its own.
      { type: "setEffectOutput", effect: tap, output: 0, target: clap },
      // Its own track, an output it lacks, a track that does not exist.
      { type: "setEffectOutput", effect: tap, output: 0, target: kick },
      { type: "setEffectOutput", effect: tap, output: 1, target: hat },
      { type: "setEffectOutput", effect: tap, output: 0, target: 9_999 },
      // The destination back into the tapped track, by output and by send.
      { type: "setTrackOutput", id: hat, output: kick },
      { type: "setSend", from: hat, to: kick, gain: 1 },
      { type: "setSidechain", from: hat, to: kick, gain: 1 },
    ] as const
    for (const command of refused) {
      await expect(app.backend.dispatch(command)).rejects.toThrow()
    }
    expect(project()).toEqual(before)
    expect(history().cursor).toBe(cursor)

    // The picker's loop test and the backend's agree.
    const open = effectOutputChoices(
      project().mixer.tracks,
      routes(),
      kick,
      tap,
      0
    )
    expect(open.tracks.map((track) => track.name)).not.toContain("Clap")
    expect(open.looping).toBe(1)
    // With the tap's route as an edge, Hat no longer may output to Kick.
    const edged = withEffectOutputEdges(project().mixer.tracks, routes())
    expect(
      edged.find((track) => track.id === kick)!.sends.map((send) => send.target)
    ).toContain(hat)
  })

  it("explains instead of offering routes on the master, and shows nothing for an effect without outputs", async () => {
    const master = trackNamed("Master").id
    const kick = trackNamed("Kick").id
    const onMaster = await addEffect(master, "sendTap")
    const plain = await addEffect(kick, "eq")
    const { unmount } = render(<Outputs track={master} effect={onMaster} />)
    expect(screen.getByText(/On the master, Send Tap has nowhere to send/))
      .toBeVisible()
    expect(screen.queryByRole("button")).toBeNull()
    await expect(
      app.backend.dispatch({
        type: "setEffectOutput",
        effect: onMaster,
        output: 0,
        target: kick,
      })
    ).rejects.toThrow()
    unmount()

    const { container } = render(<Outputs track={kick} effect={plain} />)
    expect(container).toBeEmptyDOMElement()
  })
})
