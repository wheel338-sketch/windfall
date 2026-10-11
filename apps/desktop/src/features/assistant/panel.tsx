import { useRef, useState } from "react"
import type { AssistantReply } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { backend } from "@/lib/ipc"
import { errorMessage } from "@/lib/ipc/backend"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { ProposalReview } from "./review"
import { closeAssistant, useAssistantWindows } from "./store"

function AssistantForm() {
  const project = useProjectStore((state) => state.project)
  const [prompt, setPrompt] = useState("")
  const [reply, setReply] = useState<AssistantReply | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [generation] = useState(getProjectGeneration)
  const request = useRef(0)
  async function ask() {
    const id = ++request.current
    setBusy(true); setError(null); setReply(null)
    try {
      const result = await backend.assistantRespond(prompt)
      if (id === request.current && generation === getProjectGeneration()) setReply(result)
    } catch (failure) { if (id === request.current) setError(errorMessage(failure)) }
    finally { if (id === request.current) setBusy(false) }
  }
  return <div className="flex flex-col gap-4">
    <FieldGroup><Field>
      <FieldLabel htmlFor="assistant-request">Ask about Windfall or request an edit</FieldLabel>
      <Textarea id="assistant-request" maxLength={4096} value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="Set tempo 120; create pattern Verse; add channel Bass" />
    </Field></FieldGroup>
    <div className="flex gap-2">
      <Button disabled={busy || !prompt.trim()} onClick={() => void ask()}>Ask offline assistant</Button>
      <Button variant="outline" disabled={!busy} onClick={() => { request.current += 1; setBusy(false) }}>Cancel response</Button>
    </div>
    <details><summary>Channel and mixer IDs</summary>
      <p>{project.channels.map((channel) => `${channel.name}: ${channel.id}`).join(" · ") || "No channels"}</p>
      <p>{project.mixer.tracks.map((track) => `${track.name}: ${track.id}`).join(" · ")}</p>
    </details>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {reply && <p role="status">{reply.answer}</p>}
    {reply?.proposal && <ProposalReview key={`${reply.proposal.stamp.revision}:${JSON.stringify(reply.proposal.commands)}`} proposal={reply.proposal} generation={generation} cancel={() => setReply(null)} />}
  </div>
}

export function AssistantPanel() {
  const open = useAssistantWindows((state) => state.assistant)
  const epoch = useAssistantWindows((state) => state.epoch)
  return <Dialog open={open} onOpenChange={(next) => { if (!next) closeAssistant() }}>
    <DialogContent className="max-h-[85vh] overflow-y-auto">
      <DialogHeader><DialogTitle>Assistant</DialogTitle><DialogDescription>Local help and deterministic offline commands. Review every edit before applying it.</DialogDescription></DialogHeader>
      {open && <AssistantForm key={epoch} />}
    </DialogContent>
  </Dialog>
}
