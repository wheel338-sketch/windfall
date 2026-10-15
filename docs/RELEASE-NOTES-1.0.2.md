Windfall 1.0.2 makes SoundFonts easy to use, for you and for AI agents, and warns when a channel is silent because it has nothing to play. If you have 1.0.1, Windfall installs this update by itself.

## Silent channels are flagged

A SoundFont player, multisampler, sample player, pad bank, text voice or sample-based granular instrument plays nothing until it has a source, and before 1.0.2 nothing said so. Now:

- the channel rack marks every such channel "no sample" (or "no SoundFont"), and clicking the mark opens its loader;
- an AI agent is told too: the instrument list marks the kinds that need a source, adding one of them warns that it will be silent, the project summary flags channels with no source, and a render lists the channels with notes that made no sound, and why.

## SoundFonts for you

- The SoundFont editor has a **Choose file…** button instead of a path box, and its presets are searchable and grouped by General MIDI family (Piano, Strings, Brass, Ensemble…).
- `.sf2` files are SoundFonts in the browser: their menu (or Enter) opens a list of their presets, and choosing one adds a channel playing it, as one undo step.
- **Settings → General MIDI SoundFont** remembers a General MIDI bank of your own. **Add channel → Add General MIDI instrument…** then offers its instruments by family. Windfall does not include a SoundFont.

## SoundFonts for AI agents

- `search_library` searches the folders in your browser (samples, SoundFonts, presets) with the browser's own search.
- `list_soundfont_presets` lists a SoundFont's presets with their General MIDI family.
- `load_soundfont_preset` loads a preset into a SoundFont channel, or with `replaceInstrument` makes another instrument channel a SoundFont player; `add_channel` takes a `soundFont` so one proposal gives a playable channel.
- `list_channel_presets` and `apply_channel_preset` use the presets in your preset library.

Every one of these is a proposal you apply or reject, as one undo step. An agent can only name files inside the folders in your browser and Windfall's factory content: other paths, `..`, and links or junctions that lead elsewhere are refused.

See docs/MCP.md for the tools.
