import type { MessageKey } from "./types"

export const actionMessages: Record<string, MessageKey> = {
  "edit.selectAll": "Select all",
  "edit.delete": "Delete",
  "edit.duplicate": "Duplicate",
  "edit.paste": "Paste",
  "edit.copy": "Copy",
  "edit.cut": "Cut",
  "options.settings": "Settings…",
  "view.commandPalette": "Command palette…",
  "pattern.rename": "Rename pattern…",
  "file.export": "Export audio…",
  "file.archive": "Export portable project archive…",
  "file.saveAs": "Save as…",
  "file.importFlp": "Import FL Studio project…",
  "file.open": "Open project…",
  "audioLogger.open": "Audio logger…",
  "file.exportMidi": "Export MIDI…",
  "file.importMidi": "Import MIDI…",
  "recording.open": "Record audio input",
  "recording.start": "Start recording",
  "recording.stop": "Stop recording and keep take",
  "recording.cancel": "Discard recording",
  "file.new": "New project",
  "file.save": "Save",
  "file.saveNewVersion": "Save new version",
  "file.cancelArchive": "Cancel project archive",
  "file.reloadSamples": "Reload missing samples",
  "edit.undo": "Undo",
  "edit.redo": "Redo",
  "edit.history": "History",
  "transport.toggle": "Play or stop",
  "transport.play": "Play",
  "transport.stop": "Stop",
  "transport.metronome": "Metronome",
  "transport.patternMode": "Play the pattern",
  "transport.songMode": "Play the song",
  "transport.toggleMode": "Switch between pattern and song",
  "tempo.tap": "Tap tempo",
  "tempo.half": "Halve the tempo",
  "tempo.double": "Double the tempo",
  "channel.add": "Add channel",
  "pattern.add": "Add pattern",
  "pattern.next": "Next pattern",
  "pattern.previous": "Previous pattern",
  "pattern.duplicate": "Duplicate pattern",
  "pattern.delete": "Delete pattern",
  "view.browser": "Browser",
  "view.mixer": "Mixer",
  "view.channelRack": "Channel rack",
  "view.playlist": "Playlist",
  "view.pianoRoll": "Piano roll",
  "view.toggleTheme": "Switch between light and dark",
  "view.resetLayout": "Reset layout",
  "engine.copyDeviceInfo": "Copy audio device info",
  "options.keymapWindfall": "Use Windfall shortcuts",
  "options.keymapFl": "Use FL Studio shortcuts",
  "help.gettingStarted": "Getting started",
  "help.shortcuts": "Keyboard shortcuts",
  "help.about": "About Windfall",
  "mixer.addTrack": "Add mixer track"
}

/**
 * A kept message. `translate` names the values that are catalog keys
 * themselves, such as a built-in effect's name, so they are translated when
 * the message is shown; "lower" also puts them in lower case. A value that
 * is itself made of translated text is kept as a function that makes it, so
 * it too is made in the language the message is shown in.
 */
export type DynamicActionMessage = {
  key: MessageKey
  values: Record<string, string | number | (() => string)>
  translate?: Readonly<Record<string, "plain" | "lower">>
}
const dynamic = new Map<string, { owner: object; message: DynamicActionMessage }>()
export function registerDynamicActionMessage(id: string, owner: object, message: DynamicActionMessage | undefined) {
  if (message) dynamic.set(id, { owner, message })
}
export function removeDynamicActionMessage(id: string, owner: object) {
  if (dynamic.get(id)?.owner === owner) dynamic.delete(id)
}
export function dynamicActionMessage(id: string) { return dynamic.get(id)?.message }
