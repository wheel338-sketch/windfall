import { create } from "zustand"

/**
 * Whether something outside the clip settings, such as the command palette,
 * asked for the Audio editor. The editor button in the clip settings opens
 * the editor and takes the request, even when the settings were hidden and
 * only mount because of it.
 */
export const useAudioEditorRequest = create(() => ({ pending: false }))

export function requestAudioEditor() {
  useAudioEditorRequest.setState({ pending: true })
}

export function takeAudioEditorRequest(): boolean {
  if (!useAudioEditorRequest.getState().pending) return false
  useAudioEditorRequest.setState({ pending: false })
  return true
}
