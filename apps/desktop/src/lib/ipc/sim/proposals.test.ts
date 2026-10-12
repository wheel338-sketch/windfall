import { describe, expect, it } from "vitest"
import { SimDocument } from "./document"
import { emptyProject } from "./project"

describe("proposal WASM authority", () => {
  it("rejects reused approval, malformed capability, oversized batch, recording and generation mismatch without edits", () => {
    const doc = SimDocument.create(emptyProject())
    try {
      const proposal = doc.assistant("set tempo 127", 9).proposal!
      const before = doc.snapshot(null)
      for (const apply of [
        () => doc.applyProposal(proposal, 10, false),
        () => doc.applyProposal(proposal, 9, true),
        () => doc.applyProposal({ ...proposal, commands: [{ type: "batch", commands: [] }] }, 9, false),
        () => doc.applyProposal({ ...proposal, commands: Array.from({ length: 129 }, () => ({ type: "addPattern" as const })) }, 9, false),
        () => doc.applyProposal({ ...proposal, commands: [{ type: "addPattern" }, { type: "updateChannel", id: 4294967295, patch: {} }] }, 9, false),
      ]) {
        expect(apply).toThrow()
        expect(doc.snapshot(null)).toEqual(before)
      }
      doc.applyProposal(proposal, 9, false)
      expect(doc.project().settings.tempoBpm).toBe(127)
      expect(doc.snapshot(null).history.cursor).toBe(1)
      const after = doc.snapshot(null)
      expect(() => doc.applyProposal(proposal, 9, false)).toThrow(/stale/)
      expect(doc.snapshot(null)).toEqual(after)
      doc.undo()
      expect(doc.project().settings).toEqual(before.project.settings)
    } finally { doc.dispose() }
  })
})
